#include "popup.h"

#include <QClipboard>
#include <QFileInfo>
#include <QGuiApplication>
#include <QQuickWindow>
#include <QScreen>
#include <QUrl>

#include <QPainterPath>

#include <KIO/OpenFileManagerWindowJob>
#include <KIO/OpenUrlJob>
#include <KWaylandExtras>
#include <KWindowEffects>
#include <LayerShellQt/Window>

#include "appearance.h"
#include "evalrun.h"
#include "settingswindow.h"

using namespace Qt::StringLiterals;

static constexpr int TopMargin = 120;

void Popup::attach(QQuickWindow *window, SettingsWindow *settings, Appearance *appearance)
{
    m_window = window;
    m_settings = settings;
    m_appearance = appearance;
    m_layer = LayerShellQt::Window::get(window);
    m_layer->setScope(u"endex"_s);
    // Top, not overlay: KWin stacks the input method's candidate window below overlay surfaces.
    m_layer->setLayer(LayerShellQt::Window::LayerTop);
    m_layer->setAnchors(LayerShellQt::Window::AnchorTop);
    m_layer->setWantsToBeOnActiveScreen(true);
    // Each show maps the surface with exclusive keyboard interactivity, so the compositor must
    // focus it (a D-Bus call from a global shortcut carries no activation token). Once focused it
    // drops to on-demand: clicking another window then takes the focus away, which hides the
    // popup, like KRunner.
    connect(window, &QWindow::activeChanged, this, [this] {
        if (m_window->isActive()) {
            m_wasActive = true;
            m_layer->setKeyboardInteractivity(LayerShellQt::Window::KeyboardInteractivityOnDemand);
        } else if (m_wasActive && m_window->isVisible()) {
            hide();
        }
    });
    // The compositor puts it on the active screen, which may be another one; and the look can
    // change while it shows (the settings read at a summon, theme.toml).
    connect(window, &QWindow::screenChanged, this, [this] {
        if (m_window->isVisible())
            fit();
    });
    connect(appearance, &Appearance::changed, this, [this] {
        if (m_window->isVisible())
            fit();
    });
    fit();
}

void Popup::fit()
{
    const QSize size = m_appearance->popupSize(m_window->screen());
    // Lower screens get a smaller gap above the popup.
    const int room = m_window->screen()->geometry().height() - size.height();
    const Fit next{size, qBound(0, room / 2, TopMargin), m_appearance->blurBehind(), m_appearance->popupRadius()};
    // A shown popup is only touched when something changed.
    if (m_window->isVisible() && next == m_fit)
        return;
    m_fit = next;
    m_window->resize(size);
    m_layer->setMargins({0, next.top, 0, 0});
    // KWin blurs what is behind the rounded panel (its Blur effect, when it is on).
    QPainterPath panel;
    panel.addRoundedRect(QRectF(QPointF(0, 0), size), next.radius, next.radius);
    KWindowEffects::enableBlurBehind(m_window, next.blur, QRegion(panel.toFillPolygon().toPolygon()));
}

void Popup::hide()
{
    m_wasActive = false;
    m_window->hide();
}

void Popup::Toggle()
{
    if (m_window->isVisible())
        hide();
    else
        Show({});
}

void Popup::Show(const QString &query)
{
    if (!m_window->isVisible()) {
        // Applied looks take effect here: size, blur.
        fit();
        m_layer->setKeyboardInteractivity(LayerShellQt::Window::KeyboardInteractivityExclusive);
        m_window->show();
        m_window->requestActivate();
    }
    Q_EMIT shown(query);
}

void Popup::open(const QString &path)
{
    launch(path, false);
}

void Popup::reveal(const QString &path)
{
    launch(path, true);
}

void Popup::copy(const QString &text)
{
    QGuiApplication::clipboard()->setText(text);
}

QString Popup::toggleCommand() const
{
    const QString endex = endexProgram();
    return (endex.isEmpty() ? u"endex"_s : endex) + u" toggle"_s;
}

void Popup::openSettings(const QString &page, bool welcome)
{
    KWaylandExtras::xdgActivationToken(m_window, u"endex-ui"_s).then(this, [this, page, welcome](const QString &token) {
        hide();
        m_settings->show(page, welcome, token);
    });
}

void Popup::ShowSettings(const QString &page)
{
    m_settings->show(page, false);
}

void Popup::ActivateSettings(const QString &page, const QString &activationToken)
{
    m_settings->show(page, false, activationToken);
}

void Popup::launch(const QString &path, bool reveal)
{
    if (!QFileInfo::exists(path)) {
        Q_EMIT failed(tr("文件已不存在：%1").arg(path));
        return;
    }
    const QUrl url = QUrl::fromLocalFile(path);
    // The popup, or the settings window (a config file with errors).
    QWindow *from = QGuiApplication::focusWindow() ? QGuiApplication::focusWindow() : m_window.data();
    KWaylandExtras::xdgActivationToken(from, u"endex-ui"_s).then(this, [this, url, reveal, from](const QString &token) {
        if (reveal) {
            KIO::highlightInFileManager({url}, token.toUtf8());
        } else {
            auto job = new KIO::OpenUrlJob(url);
            job->setStartupId(token.toUtf8());
            job->start();
        }
        if (from == m_window)
            hide();
    });
}
