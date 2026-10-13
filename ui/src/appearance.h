// The look of both windows, live (「外观」): the applied [appearance] settings and
// ~/.config/endex/theme.toml become a colour scheme and a font for this process only, plus the
// shapes the QML reads (Look.qml). Under 跟随 Plasma nothing is touched.

#pragma once

#include <QColor>
#include <QFont>
#include <QFileSystemWatcher>
#include <QObject>
#include <QQuickItem>
#include <QSize>
#include <QTimer>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

#include <KConfigWatcher>

#include "look.h"

class QJSEngine;
class QQmlEngine;
class QScreen;
class Settings;

class Appearance : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    // Endex's own spacing (comfortable 1, compact 0.72), corner radius and popup opacity.
    Q_PROPERTY(double density READ density NOTIFY changed)
    Q_PROPERTY(int radius READ radius NOTIFY changed)
    Q_PROPERTY(double opacity READ opacity NOTIFY changed)
    // A font of the look's own (else Kirigami's fonts are Plasma's), and its small size, which
    // Kirigami.Theme.smallFont does not follow.
    Q_PROPERTY(bool customFont READ customFont NOTIFY changed)
    Q_PROPERTY(QFont smallFont READ smallFont NOTIFY changed)
    // The popup's border; invalid: derived from the theme, as before.
    Q_PROPERTY(QColor border READ border NOTIFY changed)
    // theme.toml: where it is, the keys it sets, and why the last edit was not loaded.
    Q_PROPERTY(QString themePath READ themePath CONSTANT)
    Q_PROPERTY(QStringList themeKeys READ themeKeys NOTIFY themeChanged)
    Q_PROPERTY(QString themeError READ themeError NOTIFY themeChanged)
    // KWin offers blur behind windows (its Blur effect is on); see checkBlur().
    Q_PROPERTY(bool blurAvailable READ blurAvailable NOTIFY blurAvailableChanged)

public:
    explicit Appearance(Settings *settings, QObject *parent = nullptr);
    // The QML singleton: the instance main() made.
    static Appearance *create(QQmlEngine *, QJSEngine *);

    double density() const { return m_look.density; }
    int radius() const { return m_look.radius; }
    double opacity() const { return m_look.opacity; }
    QColor border() const { return m_look.border; }
    bool customFont() const { return m_fontSet; }
    QFont smallFont() const;
    QString themePath() const;
    QStringList themeKeys() const { return m_file.keys; }
    QString themeError() const { return m_themeError; }
    bool blurAvailable() const { return m_blurAvailable; }

    // The popup on `screen`: its size (it always fits), and the blur behind it.
    QSize popupSize(QScreen *screen) const;
    bool blurBehind() const { return m_look.blur && m_look.opacity < 1; }
    int popupRadius() const { return m_look.radius * 2; }

    // What the `appearance.*` keys of `draft` would look like, for the page's preview: the
    // colours, font, shape and the popup's size on `item`'s screen.
    Q_INVOKABLE QVariantMap preview(const QVariantMap &draft, QQuickItem *item) const;
    // The `appearance.*` keys a preset decides; empty for "custom".
    Q_INVOKABLE QVariantMap presetValues(const QString &preset) const;
    // The installed family of a font choice ("noto_sans", …); empty when none is.
    Q_INVOKABLE QString installedFamily(const QString &font) const;
    Q_INVOKABLE QString sampleToml(const QVariantMap &draft, QQuickItem *item) const;
    // Asks the compositor again whether blur is offered (read-only).
    Q_INVOKABLE void checkBlur();
    // System Settings on Desktop Effects, where KWin's Blur is switched on and its strength set.
    Q_INVOKABLE void openEffectSettings(QQuickItem *from);

Q_SIGNALS:
    void changed();
    void themeChanged();
    void blurAvailableChanged();
    void effectSettingsFailed(const QString &message);

private:
    look::Look lookOf(const QVariantMap &settings) const;
    QStringList installed(const QStringList &families) const;
    void update();
    void readSystem();
    void readThemeFile();
    void watchThemeFile();
    void applyColors();
    void applyFont();

    Settings *m_settings;
    // The applied settings the look was made from.
    QVariantMap m_saved;
    // Plasma's colours (kdeglobals, read-only), also while this process has its own.
    look::ColorSets m_system;
    bool m_systemDark = false;
    KConfigWatcher::Ptr m_globals;
    // The last theme.toml that loaded.
    look::ThemeFile m_file;
    QString m_themeError;
    QFileSystemWatcher m_watcher;
    QTimer m_reread;
    look::Look m_look;
    // The colour scheme file in use; empty: Plasma's.
    QString m_schemePath;
    bool m_fontSet = false;
    bool m_blurAvailable = false;
};
