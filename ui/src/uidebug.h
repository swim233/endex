// Test hook for the nested-KWin checks (ENDEX_UI_DEBUG=1), `org.endex.UiDebug`: reports the
// popup's and the settings window's state, and stands in for what the tests cannot drive
// (window decorations, and the folder dialog, which goes through the portal and would show the
// real home folder).

#pragma once

#include <QObject>

class SettingsWindow;

class UiDebug : public QObject
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.endex.UiDebug")

public:
    UiDebug(QObject *popup, SettingsWindow *settings);

public Q_SLOTS:
    Q_SCRIPTABLE QString State();
    Q_SCRIPTABLE QString SettingsState();
    // As if the folder dialog had returned `path`.
    Q_SCRIPTABLE QString AddFolder(const QString &path);
    // Clicks the button (or triggers the action) with this objectName in either window.
    Q_SCRIPTABLE bool Click(const QString &objectName);
    // Types `text` into the field with this objectName, focused as by the user, and presses
    // Enter in a single-line field.
    Q_SCRIPTABLE bool SetText(const QString &objectName, const QString &text);
    // Sets a slider's or spin box's value as a drag or an edit would.
    Q_SCRIPTABLE bool SetValue(const QString &objectName, double value);
    // Scrolls a ScrollView to `fraction` (0 top, 1 bottom) of its content.
    Q_SCRIPTABLE bool Scroll(const QString &objectName, double fraction);
    // Closes the settings window as its close button would.
    Q_SCRIPTABLE void CloseSettings();

private:
    // A visible, enabled object with this name in either window.
    QObject *find(const QString &objectName) const;

    QObject *m_popup;
    SettingsWindow *m_settings;
};
