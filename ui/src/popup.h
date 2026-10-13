// The popup window: org.endex.Ui1 on D-Bus, layer-shell placement and focus, desktop actions.

#pragma once

#include <QObject>
#include <QPointer>
#include <QSize>
#include <QtQml/qqmlregistration.h>

class Appearance;
class QQuickWindow;
class SettingsWindow;
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
    void attach(QQuickWindow *window, SettingsWindow *settings, Appearance *appearance);

    Q_INVOKABLE void hide();
    // Default application; hides the popup if it asked.
    Q_INVOKABLE void open(const QString &path);
    // File manager with the file selected (org.freedesktop.FileManager1.ShowItems); hides.
    Q_INVOKABLE void reveal(const QString &path);
    Q_INVOKABLE void copy(const QString &text);
    // What a global shortcut should run: `<absolute path>/endex toggle`.
    Q_INVOKABLE QString toggleCommand() const;
    // The settings window on `page` (empty: where it was); `welcome` for the first run. It
    // takes the popup's focus.
    Q_INVOKABLE void openSettings(const QString &page, bool welcome);

public Q_SLOTS:
    Q_SCRIPTABLE void Toggle();
    Q_SCRIPTABLE void Show(const QString &query);
    // The settings window (`endex settings [page]`).
    Q_SCRIPTABLE void ShowSettings(const QString &page);
    // The same with the caller's XDG activation token (`endex settings` started from the app
    // menu has one), which lets the compositor bring an open window to the front.
    Q_SCRIPTABLE void ActivateSettings(const QString &page, const QString &activationToken);

Q_SIGNALS:
    // After the window is mapped; QML focuses the search field.
    void shown(const QString &query);
    // An action could not run; the popup stays open to say why.
    void failed(const QString &message);

private:
    // Hands the default application or file manager an activation token, then hides the popup
    // if the action came from it.
    void launch(const QString &path, bool reveal);
    // Size and blur for the screen it is on; the top margin shrinks on low screens.
    void fit();

    QPointer<QQuickWindow> m_window;
    SettingsWindow *m_settings = nullptr;
    Appearance *m_appearance = nullptr;
    // What fit() last applied.
    struct Fit {
        QSize size;
        int top = 0;
        bool blur = false;
        int radius = 0;
        bool operator==(const Fit &) const = default;
    } m_fit;
    LayerShellQt::Window *m_layer = nullptr;
    bool m_wasActive = false;
};
