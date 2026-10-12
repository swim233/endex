#include "settingswindow.h"

#include <QQmlComponent>
#include <QQmlEngine>
#include <QQuickWindow>

using namespace Qt::StringLiterals;

SettingsWindow::SettingsWindow(QQmlEngine *engine, const QVariantMap &properties, QObject *parent)
    : QObject(parent)
    , m_engine(engine)
    , m_properties(properties)
{
}

QQuickWindow *SettingsWindow::window() const
{
    return m_window;
}

void SettingsWindow::show(const QString &page, bool welcome, const QString &activationToken)
{
    if (!m_window) {
        QQmlComponent component(m_engine, u"Endex"_s, u"SettingsWindow"_s);
        m_window = qobject_cast<QQuickWindow *>(component.createWithInitialProperties(m_properties));
        if (!m_window) {
            qWarning().noquote() << "endex-ui: cannot create the settings window:" << component.errorString();
            return;
        }
    }
    QMetaObject::invokeMethod(m_window, "showPage", Q_ARG(QVariant, page), Q_ARG(QVariant, welcome));
    // Asked without a valid activation token (a terminal, a script), KWin leaves an open window
    // behind the active one, but it focuses a window that maps anew: the user did just ask for
    // it. A token, when there is one, goes with the new mapping.
    if (m_window->isVisible() && !m_window->isActive())
        m_window->hide();
    if (!activationToken.isEmpty())
        qputenv("XDG_ACTIVATION_TOKEN", activationToken.toUtf8());
    if (m_window->windowState() & Qt::WindowMinimized)
        m_window->showNormal();
    else
        m_window->show();
    m_window->raise();
}
