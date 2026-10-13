// The appearance as data: the [appearance] settings and their presets, theme.toml, the built-in
// palettes, contrast, the popup's size on a screen and the colour scheme it all becomes. No
// application state; Appearance applies the result.

#pragma once

#include <QColor>
#include <QMap>
#include <QSize>
#include <QString>
#include <QStringList>
#include <QVariantMap>

#include <optional>

namespace look
{

// The [appearance] keys of config.toml (GetSettings: `appearance.*`), defaults as in endexd.
struct Settings {
    QString preset = QStringLiteral("plasma");
    QString base = QStringLiteral("plasma");
    QString scheme = QStringLiteral("system");
    QString accent = QStringLiteral("default");
    QString font = QStringLiteral("system");
    double fontSize = 10;
    QString density = QStringLiteral("comfortable");
    int radius = 5;
    QString popupSize = QStringLiteral("default");
    int screenPercent = 60;
    int popupWidth = 1100;
    int popupHeight = 640;
    int translucency = 0;
    bool blur = false;

    // Missing or unknown values keep the default.
    static Settings fromMap(const QVariantMap &settings);
    // With the preset's style keys in place (none for "custom").
    Settings effective() const;
};

// The style keys a preset decides, as `appearance.*` settings keys; empty for "custom".
QVariantMap presetValues(const QString &preset);

// What ~/.config/endex/theme.toml sets; everything is optional.
struct ThemeFile {
    std::optional<QString> base;
    std::optional<QString> scheme;
    // accent, window, view, text, muted, line, positive, neutral, negative
    QMap<QString, QColor> colors;
    // Empty: the system font.
    std::optional<QString> family;
    std::optional<double> sizePt;
    std::optional<int> radius;
    std::optional<QString> density;
    std::optional<int> width;
    std::optional<int> height;
    std::optional<double> opacity;
    std::optional<bool> blur;
    // The keys it sets, as `table.key`.
    QStringList keys;

    // The TOML subset the file uses: tables, `key = value` with strings, numbers and booleans,
    // comments. Unknown tables and keys are errors. `error` gets 「第 N 行：…」.
    static std::optional<ThemeFile> parse(const QString &text, QString *error);
};

// A KColorScheme colour set ("Window", "View", …): key ("BackgroundNormal", …) → colour.
using ColorSet = QMap<QString, QColor>;
using ColorSets = QMap<QString, ColorSet>;

// WCAG contrast ratio.
double contrast(const QColor &a, const QColor &b);
QColor mix(const QColor &a, const QColor &b, double t);
// `color` moved towards `toward` until it reaches `min` against `background`.
QColor ensureContrast(const QColor &color, const QColor &background, double min, const QColor &toward);
// Dark or white, whichever reads better on `color`.
QColor inkOn(const QColor &color);

// Installed-or-not candidates for a font choice ("noto_sans", …); empty for "system".
QStringList fontCandidates(const QString &font);

struct Look {
    // The palette ("plasma" or "proto") and the 明暗 setting ("system", "light" or "dark").
    QString base = QStringLiteral("plasma");
    QString schemeSetting = QStringLiteral("system");
    // What is shown: "light" or "dark".
    QString scheme;
    // Plasma's own colours, untouched: no colour scheme to apply.
    bool systemColors = true;
    // The colour scheme (all sets), also when systemColors.
    ColorSets sets;
    QColor window, view, alternate, text, muted, accent, accentText, accentInk, positive, neutral, negative;
    // The popup's border; invalid: derived from the theme as before.
    QColor border;
    // The accent is hard to tell from the window colour.
    bool accentWarning = false;
    // Plasma's font, family and size.
    bool systemFont = true;
    // Families to try, in order; empty: the system family.
    QStringList families;
    double fontSize = 10;
    double density = 1;
    int radius = 5;
    double opacity = 1;
    bool blur = false;
    // "default", "small", "screen" or "custom", with the numbers they use.
    QString popupSize = QStringLiteral("default");
    int screenPercent = 60;
    int popupWidth = 1100;
    int popupHeight = 640;
};

// `system`: Plasma's current colour sets; `systemDark`: whether they are dark.
Look resolve(const Settings &settings, const ThemeFile &file, const ColorSets &system, bool systemDark);

struct PopupSize {
    QSize size;
    // What the settings ask for, before it was made to fit.
    QSize wanted;
    bool clamped = false;
};
// The popup on a screen of this (logical) size: it always fits, leaving 60 px above and below
// for panels and 16 px at the sides. Wayland does not tell clients where panels are or how big.
PopupSize popupSize(const Look &look, QSize screen);

// The colour scheme as a .colors file.
QString schemeText(const Look &look);
// A theme.toml that pins the look down, for the 「主题文件」 section; `popup`: the popup's size
// as asked for.
QString sampleToml(const Look &look, QSize popup);

}
