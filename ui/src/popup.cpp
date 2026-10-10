#include "popup.h"

#include <QClipboard>
#include <QCoreApplication>
#include <QFileInfo>
#include <QGuiApplication>
#include <QProcess>
#include <QQuickWindow>
#include <QScreen>
#include <QStandardPaths>
#include <QUrl>

#include <KIO/OpenFileManagerWindowJob>
#include <KIO/OpenUrlJob>
#include <KWaylandExtras>
#include <LayerShellQt/Window>

using namespace Qt::StringLiterals;

static constexpr int TopMargin = 120;

void Popup::attach(QQuickWindow *window)
{
    m_window = window;
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
        // Lower screens get a smaller gap above the popup.
        const int room = m_window->screen()->geometry().height() - m_window->height();
        m_layer->setMargins({0, qBound(0, room / 2, TopMargin), 0, 0});
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

void Popup::openConfig(const QString &path)
{
    const QString file = path.isEmpty()
        ? QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation) + u"/endex/config.toml"_s
        : path;
    const auto open = [this, file] {
        if (QFileInfo::exists(file))
            launch(file, false);
        else
            Q_EMIT failed(tr("还没有配置文件 %1：运行一次 endex 或启动守护进程就会创建").arg(file));
    };
    const QString sibling = QCoreApplication::applicationDirPath() + u"/endex"_s;
    const QString endex = QFileInfo(sibling).isExecutable() ? sibling : QStandardPaths::findExecutable(u"endex"_s);
    if (QFileInfo::exists(file) || endex.isEmpty()) {
        open();
        return;
    }
    // Any `endex` command writes the commented default config on first use.
    auto config = new QProcess(this);
    config->setStandardOutputFile(QProcess::nullDevice());
    connect(config, &QProcess::finished, this, [config, open] {
        config->deleteLater();
        open();
    });
    connect(config, &QProcess::errorOccurred, this, [config, open](QProcess::ProcessError error) {
        if (error != QProcess::FailedToStart)
            return;
        config->deleteLater();
        open();
    });
    config->start(endex, {u"config"_s, u"data_dir"_s});
}

void Popup::launch(const QString &path, bool reveal)
{
    if (!QFileInfo::exists(path)) {
        Q_EMIT failed(tr("文件已不存在：%1").arg(path));
        return;
    }
    const QUrl url = QUrl::fromLocalFile(path);
    KWaylandExtras::xdgActivationToken(m_window, u"endex-ui"_s).then(this, [this, url, reveal](const QString &token) {
        if (reveal) {
            KIO::highlightInFileManager({url}, token.toUtf8());
        } else {
            auto job = new KIO::OpenUrlJob(url);
            job->setStartupId(token.toUtf8());
            job->start();
        }
        hide();
    });
}
