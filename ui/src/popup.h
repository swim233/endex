// The popup window: org.endex.Ui1 on D-Bus, layer-shell placement and focus, desktop actions.

#pragma once

#include <QObject>
#include <QPointer>
#include <QtQml/qqmlregistration.h>

class QQuickWindow;
namespace LayerShellQt
{
class Window;
}

class Popup : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_UNCREATABLE("Created by main")
    Q_CLASSINFO("D-Bus Interface", "org.endex.Ui1")

public:
    void attach(QQuickWindow *window);

    Q_INVOKABLE void hide();
    // Default application; hides the popup.
    Q_INVOKABLE void open(const QString &path);
    // File manager with the file selected (org.freedesktop.FileManager1.ShowItems); hides.
    Q_INVOKABLE void reveal(const QString &path);
    Q_INVOKABLE void copy(const QString &text);
    // The daemon's config file, or the default location; creates the default config first if
    // there is none yet (via `endex config`).
    Q_INVOKABLE void openConfig(const QString &path);

public Q_SLOTS:
    Q_SCRIPTABLE void Toggle();
    Q_SCRIPTABLE void Show(const QString &query);

Q_SIGNALS:
    // After the window is mapped; QML focuses the search field.
    void shown(const QString &query);
    // An action could not run; the popup stays open to say why.
    void failed(const QString &message);

private:
    // Hands the default application or file manager an activation token, then hides.
    void launch(const QString &path, bool reveal);

    QPointer<QQuickWindow> m_window;
    LayerShellQt::Window *m_layer = nullptr;
    bool m_wasActive = false;
};
