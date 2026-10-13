#include "look.h"

#include <QObject>
#include <QRegularExpression>

#include <cmath>

using namespace Qt::StringLiterals;

namespace look
{

namespace
{

// The palettes of the appearance demo: Breeze-like ("plasma") and the original mockup's
// ("proto"), light and dark.
struct Palette {
    QColor window, view, alternate, text, muted, line, accent, accentText, button, positive, neutral, negative, header, border;
};

Palette palette(const QString &base, bool dark)
{
    auto c = [](const char *hex) {
        return QColor(QLatin1StringView(hex));
    };
    if (base == u"proto"_s) {
        if (dark)
            return {c("#0f1412"), c("#161d1a"), c("#1c2421"), c("#e3eae5"), c("#98a79f"), c("#2b3732"), c("#62c3a5"),
                    c("#62c3a5"), c("#1c2421"), c("#62c3a5"), c("#e8a765"), c("#e57d69"), c("#1c2421"), c("#2b3732")};
        return {c("#f1f3f2"), c("#fbfcfb"), c("#e6e9e7"), c("#1b2320"), c("#56635d"), c("#d3d9d5"), c("#1d6b57"),
                c("#1d6b57"), c("#fbfcfb"), c("#1d6b57"), c("#9a4a0b"), c("#a23b2a"), c("#e6e9e7"), c("#c3cac6")};
    }
    if (dark)
        return {c("#31363b"), c("#1b1e20"), c("#3a4045"), c("#fcfcfc"), c("#a5aeb5"), c("#4b5258"), c("#3daee9"),
                c("#5cbdf0"), c("#3b4045"), c("#4cc582"), c("#f0a04b"), c("#ef6b78"), c("#2a2e32"), c("#4b5258")};
    return {c("#eff0f1"), c("#fcfcfc"), c("#e3e5e7"), c("#232627"), c("#5d686d"), c("#c9ced1"), c("#3daee9"),
            c("#0f78b8"), c("#e7e9ea"), c("#1f8a4c"), c("#b85400"), c("#c4303f"), c("#dfe1e3"), c("#b9bfc3")};
}

const QStringList SetNames = {u"View"_s, u"Window"_s, u"Button"_s, u"Selection"_s, u"Tooltip"_s, u"Complementary"_s, u"Header"_s};

// Every set of a palette, in the roles of a KColorScheme file.
ColorSets setsOf(const Palette &p, const Palette &complement, const QColor &ink)
{
    auto foregrounds = [](const Palette &p) {
        return ColorSet{
            {u"ForegroundNormal"_s, p.text},
            {u"ForegroundInactive"_s, p.muted},
            {u"ForegroundActive"_s, p.accentText},
            {u"ForegroundLink"_s, p.accentText},
            {u"ForegroundVisited"_s, mix(p.accentText, p.text, 0.35)},
            {u"ForegroundNegative"_s, p.negative},
            {u"ForegroundNeutral"_s, p.neutral},
            {u"ForegroundPositive"_s, p.positive},
            {u"DecorationFocus"_s, p.accent},
            {u"DecorationHover"_s, p.accent},
        };
    };
    auto set = [&](const Palette &p, const QColor &background, const QColor &alternate) {
        ColorSet s = foregrounds(p);
        s.insert(u"BackgroundNormal"_s, background);
        s.insert(u"BackgroundAlternate"_s, alternate);
        return s;
    };
    ColorSets sets;
    sets[u"View"_s] = set(p, p.view, mix(p.view, p.alternate, 0.5));
    sets[u"Window"_s] = set(p, p.window, p.alternate);
    sets[u"Button"_s] = set(p, p.button, p.alternate);
    sets[u"Tooltip"_s] = set(p, p.window, p.alternate);
    sets[u"Header"_s] = set(p, p.header, p.window);
    sets[u"Complementary"_s] = set(complement, complement.window, complement.alternate);
    ColorSet selection = set(p, p.accent, mix(p.accent, p.view, 0.4));
    for (const auto &key : {u"ForegroundNormal"_s, u"ForegroundActive"_s, u"ForegroundLink"_s, u"ForegroundVisited"_s})
        selection.insert(key, ink);
    selection.insert(u"ForegroundInactive"_s, mix(ink, p.accent, 0.3));
    sets[u"Selection"_s] = selection;
    return sets;
}

void setAll(ColorSets &sets, const QString &key, const QColor &color, const QStringList &except = {})
{
    for (const QString &name : SetNames) {
        if (!except.contains(name))
            sets[name].insert(key, color);
    }
}

double luminance(const QColor &c)
{
    auto channel = [](double v) {
        return v <= 0.03928 ? v / 12.92 : std::pow((v + 0.055) / 1.055, 2.4);
    };
    return 0.2126 * channel(c.redF()) + 0.7152 * channel(c.greenF()) + 0.0722 * channel(c.blueF());
}

QString hex(const QColor &c)
{
    return c.name(QColor::HexRgb);
}

} // namespace

Settings Settings::fromMap(const QVariantMap &map)
{
    Settings s;
    auto text = [&map](const char *key, QString &into, std::initializer_list<const char *> allowed) {
        const QString value = map.value(u"appearance."_s + QLatin1StringView(key)).toString();
        for (const char *a : allowed) {
            if (value == QLatin1StringView(a))
                into = value;
        }
    };
    auto number = [&map](const char *key, int &into, int min, int max) {
        const QVariant value = map.value(u"appearance."_s + QLatin1StringView(key));
        if (value.isValid())
            into = qBound(min, value.toInt(), max);
    };
    text("preset", s.preset, {"plasma", "proto", "glass", "custom"});
    text("base", s.base, {"plasma", "proto"});
    text("scheme", s.scheme, {"system", "light", "dark"});
    text("font", s.font, {"system", "noto_sans", "inter", "noto_serif", "jetbrains_mono"});
    text("density", s.density, {"comfortable", "compact"});
    text("popup_size", s.popupSize, {"default", "small", "screen", "custom"});
    const QString accent = map.value(u"appearance.accent"_s).toString();
    if (accent == u"default"_s || QRegularExpression(u"^#[0-9a-fA-F]{6}$"_s).match(accent).hasMatch())
        s.accent = accent.toLower();
    if (const QVariant size = map.value(u"appearance.font_size"_s); size.isValid())
        s.fontSize = qBound(8.0, std::round(size.toDouble() * 2) / 2, 14.0);
    number("radius", s.radius, 0, 16);
    number("screen_percent", s.screenPercent, 40, 90);
    number("popup_width", s.popupWidth, 480, 1600);
    number("popup_height", s.popupHeight, 320, 1000);
    number("translucency", s.translucency, 0, 60);
    if (const QVariant blur = map.value(u"appearance.blur"_s); blur.isValid())
        s.blur = blur.toBool();
    return s;
}

QVariantMap presetValues(const QString &preset)
{
    QVariantMap v{
        {u"appearance.base"_s, u"plasma"_s},
        {u"appearance.accent"_s, u"default"_s},
        {u"appearance.font"_s, u"system"_s},
        {u"appearance.font_size"_s, 10.0},
        {u"appearance.density"_s, u"comfortable"_s},
        {u"appearance.radius"_s, 5},
        {u"appearance.popup_size"_s, u"default"_s},
        {u"appearance.translucency"_s, 0},
        {u"appearance.blur"_s, false},
    };
    if (preset == u"proto"_s) {
        v[u"appearance.base"_s] = u"proto"_s;
        v[u"appearance.font"_s] = u"noto_sans"_s;
        v[u"appearance.font_size"_s] = 10.5;
        v[u"appearance.radius"_s] = 8;
    } else if (preset == u"glass"_s) {
        v[u"appearance.radius"_s] = 10;
        v[u"appearance.translucency"_s] = 35;
        v[u"appearance.blur"_s] = true;
    } else if (preset != u"plasma"_s) {
        return {};
    }
    return v;
}

Settings Settings::effective() const
{
    const QVariantMap values = presetValues(preset);
    if (values.isEmpty())
        return *this;
    QVariantMap map;
    for (auto it = values.cbegin(); it != values.cend(); ++it)
        map.insert(it.key(), it.value());
    Settings s = fromMap(map);
    s.preset = preset;
    s.scheme = scheme;
    s.screenPercent = screenPercent;
    s.popupWidth = popupWidth;
    s.popupHeight = popupHeight;
    return s;
}

std::optional<ThemeFile> ThemeFile::parse(const QString &text, QString *error)
{
    static const QMap<QString, QStringList> Known{
        {u"theme"_s, {u"base"_s, u"scheme"_s}},
        {u"colors"_s, {u"accent"_s, u"window"_s, u"view"_s, u"text"_s, u"muted"_s, u"line"_s, u"positive"_s, u"neutral"_s, u"negative"_s}},
        {u"font"_s, {u"family"_s, u"size_pt"_s}},
        {u"shape"_s, {u"radius"_s, u"density"_s}},
        {u"popup"_s, {u"width"_s, u"height"_s, u"opacity"_s, u"blur"_s}},
    };
    static const QRegularExpression Header(u"^\\[\\s*([A-Za-z0-9_-]+)\\s*\\]\\s*(#.*)?$"_s);
    static const QRegularExpression Key(u"^([A-Za-z0-9_-]+)\\s*=\\s*(.*)$"_s);
    static const QRegularExpression Number(u"^([+-]?[0-9]+(\\.[0-9]+)?)\\s*(#.*)?$"_s);
    static const QRegularExpression Bool(u"^(true|false)\\s*(#.*)?$"_s);
    static const QRegularExpression Color(u"^#([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$"_s);

    ThemeFile file;
    QString table;
    const QStringList lines = text.split(u'\n');
    for (int i = 0; i < lines.size(); ++i) {
        const QString line = lines[i].trimmed();
        auto fail = [&](const QString &message) {
            if (error)
                *error = QObject::tr("第 %1 行：%2").arg(i + 1).arg(message);
            return std::nullopt;
        };
        if (line.isEmpty() || line.startsWith(u'#'))
            continue;
        if (line.startsWith(u'[')) {
            const auto m = Header.match(line);
            if (!m.hasMatch())
                return fail(QObject::tr("看不懂这一节的标题"));
            table = m.captured(1);
            if (!Known.contains(table))
                return fail(QObject::tr("没有 [%1] 这一节（可以用 theme、colors、font、shape、popup）").arg(table));
            continue;
        }
        const auto m = Key.match(line);
        if (!m.hasMatch())
            return fail(QObject::tr("应为「键 = 值」"));
        const QString key = m.captured(1);
        if (table.isEmpty())
            return fail(QObject::tr("%1 要写在某一节里，例如 [colors]").arg(key));
        if (!Known[table].contains(key))
            return fail(QObject::tr("[%1] 里没有 %2 这个键").arg(table, key));
        const QString name = table + u'.' + key;
        if (file.keys.contains(name))
            return fail(QObject::tr("%1 写了两次").arg(name));
        file.keys.append(name);

        // The value: a string, a number or a boolean, then an optional comment.
        const QString raw = m.captured(2);
        std::optional<QString> string;
        std::optional<double> number;
        std::optional<bool> boolean;
        if (raw.startsWith(u'"') || raw.startsWith(u'\'')) {
            const QChar quote = raw[0];
            QString value;
            int j = 1;
            bool closed = false;
            for (; j < raw.size(); ++j) {
                const QChar c = raw[j];
                if (c == quote) {
                    closed = true;
                    break;
                }
                if (c == u'\\' && quote == u'"' && j + 1 < raw.size()) {
                    const QChar e = raw[++j];
                    if (e == u'n')
                        value += u'\n';
                    else if (e == u't')
                        value += u'\t';
                    else if (e == u'u' && j + 4 < raw.size())
                        value += QChar(raw.mid(j + 1, 4).toUShort(nullptr, 16)), j += 4;
                    else
                        value += e;
                    continue;
                }
                value += c;
            }
            const QString rest = raw.mid(j + 1).trimmed();
            if (!closed || !(rest.isEmpty() || rest.startsWith(u'#')))
                return fail(QObject::tr("字符串没有正确结束"));
            string = value;
        } else if (const auto n = Number.match(raw); n.hasMatch()) {
            number = n.captured(1).toDouble();
        } else if (const auto b = Bool.match(raw); b.hasMatch()) {
            boolean = b.captured(1) == u"true"_s;
        } else {
            return fail(QObject::tr("%1 的值应为字符串、数字或 true / false").arg(name));
        }

        auto choice = [&](std::optional<QString> &into, const QStringList &allowed) -> bool {
            if (!string || !allowed.contains(*string))
                return false;
            into = *string;
            return true;
        };
        auto ranged = [&](double min, double max) -> bool {
            return number && *number >= min && *number <= max;
        };
        bool ok = true;
        QString expected;
        if (table == u"theme"_s && key == u"base"_s) {
            ok = choice(file.base, {u"plasma"_s, u"proto"_s});
            expected = u"\"plasma\" 或 \"proto\""_s;
        } else if (table == u"theme"_s) {
            ok = choice(file.scheme, {u"system"_s, u"light"_s, u"dark"_s});
            expected = u"\"system\"、\"light\" 或 \"dark\""_s;
        } else if (table == u"colors"_s) {
            ok = string && Color.match(*string).hasMatch();
            if (ok)
                file.colors.insert(key, QColor(*string));
            expected = QObject::tr("\"#rrggbb\" 形式的颜色");
        } else if (key == u"family"_s) {
            ok = string.has_value();
            file.family = string.value_or(QString()).trimmed();
            expected = QObject::tr("字体名（空字符串表示系统字体）");
        } else if (key == u"size_pt"_s) {
            ok = ranged(8, 14);
            file.sizePt = number;
            expected = QObject::tr("8 到 14 之间的数字");
        } else if (key == u"radius"_s) {
            ok = ranged(0, 16);
            file.radius = qRound(number.value_or(0));
            expected = QObject::tr("0 到 16 之间的数字");
        } else if (key == u"density"_s) {
            ok = choice(file.density, {u"comfortable"_s, u"compact"_s});
            expected = u"\"comfortable\" 或 \"compact\""_s;
        } else if (key == u"width"_s) {
            ok = ranged(480, 1600);
            file.width = qRound(number.value_or(0));
            expected = QObject::tr("480 到 1600 之间的数字");
        } else if (key == u"height"_s) {
            ok = ranged(320, 1000);
            file.height = qRound(number.value_or(0));
            expected = QObject::tr("320 到 1000 之间的数字");
        } else if (key == u"opacity"_s) {
            ok = ranged(0.4, 1);
            file.opacity = number;
            expected = QObject::tr("0.4 到 1 之间的数字");
        } else {
            ok = boolean.has_value();
            file.blur = boolean;
            expected = u"true 或 false"_s;
        }
        if (!ok)
            return fail(QObject::tr("%1 应为 %2").arg(name, expected));
    }
    return file;
}

double contrast(const QColor &a, const QColor &b)
{
    const double x = luminance(a);
    const double y = luminance(b);
    return (std::max(x, y) + 0.05) / (std::min(x, y) + 0.05);
}

QColor mix(const QColor &a, const QColor &b, double t)
{
    auto channel = [t](int x, int y) {
        return qBound(0, int(std::lround(x + (y - x) * t)), 255);
    };
    return QColor(channel(a.red(), b.red()), channel(a.green(), b.green()), channel(a.blue(), b.blue()));
}

QColor ensureContrast(const QColor &color, const QColor &background, double min, const QColor &toward)
{
    QColor c = color;
    for (int i = 0; i < 14 && contrast(c, background) < min; ++i)
        c = mix(c, toward, 0.12);
    return c;
}

QColor inkOn(const QColor &color)
{
    const QColor dark(u"#0b1115"_s);
    const QColor white(Qt::white);
    return contrast(dark, color) >= contrast(white, color) ? dark : white;
}

QStringList fontCandidates(const QString &font)
{
    if (font == u"noto_sans"_s)
        return {u"Noto Sans SC"_s, u"Noto Sans CJK SC"_s, u"Source Han Sans SC"_s};
    if (font == u"inter"_s)
        return {u"Inter"_s};
    if (font == u"noto_serif"_s)
        return {u"Noto Serif SC"_s, u"Noto Serif CJK SC"_s, u"Source Han Serif SC"_s};
    if (font == u"jetbrains_mono"_s)
        return {u"JetBrains Mono"_s};
    return {};
}

Look resolve(const Settings &settings, const ThemeFile &file, const ColorSets &system, bool systemDark)
{
    const Settings s = settings.effective();
    Look look;
    look.base = file.base.value_or(s.base);
    look.schemeSetting = file.scheme.value_or(s.scheme);
    const bool dark = look.schemeSetting == u"dark"_s || (look.schemeSetting == u"system"_s && systemDark);
    look.scheme = dark ? u"dark"_s : u"light"_s;

    // Plasma's own colours, or a built-in palette; then the accent and the file's colours.
    const bool plasma = look.base == u"plasma"_s && look.schemeSetting == u"system"_s;
    const QColor accent = file.colors.value(u"accent"_s, s.accent == u"default"_s ? QColor() : QColor(s.accent));
    look.systemColors = plasma && !accent.isValid() && file.colors.isEmpty();
    if (plasma) {
        look.sets = system;
    } else {
        const Palette p = palette(look.base, dark);
        look.sets = setsOf(p, palette(look.base, true), inkOn(p.accent));
        look.border = p.border;
    }
    ColorSets &sets = look.sets;
    const QStringList own = {u"Selection"_s, u"Complementary"_s};
    const QColor window = file.colors.value(u"window"_s);
    const QColor view = file.colors.value(u"view"_s);
    const QColor text = file.colors.value(u"text"_s);
    if (window.isValid()) {
        for (const auto &name : {u"Window"_s, u"Tooltip"_s, u"Header"_s})
            sets[name].insert(u"BackgroundNormal"_s, window);
        sets[u"Header"_s].insert(u"BackgroundAlternate"_s, window);
    }
    if (view.isValid())
        sets[u"View"_s].insert(u"BackgroundNormal"_s, view);
    if (text.isValid())
        setAll(sets, u"ForegroundNormal"_s, text, own);
    if (window.isValid() || view.isValid() || text.isValid()) {
        // The shades in between follow the colours the file sets.
        const QColor w = sets[u"Window"_s][u"BackgroundNormal"_s];
        const QColor v = sets[u"View"_s][u"BackgroundNormal"_s];
        const QColor t = sets[u"Window"_s][u"ForegroundNormal"_s];
        sets[u"Window"_s].insert(u"BackgroundAlternate"_s, mix(w, t, 0.07));
        sets[u"Tooltip"_s].insert(u"BackgroundAlternate"_s, mix(w, t, 0.07));
        sets[u"Button"_s].insert(u"BackgroundNormal"_s, mix(w, t, 0.04));
        sets[u"Button"_s].insert(u"BackgroundAlternate"_s, mix(w, t, 0.07));
        sets[u"View"_s].insert(u"BackgroundAlternate"_s, mix(v, t, 0.03));
    }
    if (const QColor muted = file.colors.value(u"muted"_s); muted.isValid())
        setAll(sets, u"ForegroundInactive"_s, muted, own);
    for (const auto &[name, key] : {std::pair{u"positive"_s, u"ForegroundPositive"_s},
                                    std::pair{u"neutral"_s, u"ForegroundNeutral"_s},
                                    std::pair{u"negative"_s, u"ForegroundNegative"_s}}) {
        if (const QColor c = file.colors.value(name); c.isValid())
            setAll(sets, key, c, {u"Complementary"_s});
    }
    look.window = sets[u"Window"_s][u"BackgroundNormal"_s];
    look.view = sets[u"View"_s][u"BackgroundNormal"_s];
    look.alternate = sets[u"Window"_s][u"BackgroundAlternate"_s];
    look.text = sets[u"Window"_s][u"ForegroundNormal"_s];
    look.muted = sets[u"Window"_s][u"ForegroundInactive"_s];
    look.positive = sets[u"Window"_s][u"ForegroundPositive"_s];
    look.neutral = sets[u"Window"_s][u"ForegroundNeutral"_s];
    look.negative = sets[u"Window"_s][u"ForegroundNegative"_s];
    if (accent.isValid()) {
        // Text in the accent stays readable; the ink on it is whichever reads better.
        const QColor accentText = ensureContrast(accent, look.view, 4.5, look.text);
        const QColor ink = inkOn(accent);
        setAll(sets, u"DecorationFocus"_s, accent);
        setAll(sets, u"DecorationHover"_s, accent);
        setAll(sets, u"ForegroundActive"_s, accentText, own);
        setAll(sets, u"ForegroundLink"_s, accentText, own);
        ColorSet &selection = sets[u"Selection"_s];
        selection.insert(u"BackgroundNormal"_s, accent);
        selection.insert(u"BackgroundAlternate"_s, mix(accent, look.view, 0.4));
        for (const auto &key : {u"ForegroundNormal"_s, u"ForegroundActive"_s, u"ForegroundLink"_s, u"ForegroundVisited"_s})
            selection.insert(key, ink);
        selection.insert(u"ForegroundInactive"_s, mix(ink, accent, 0.3));
    }
    look.accent = sets[u"Selection"_s][u"BackgroundNormal"_s];
    look.accentInk = sets[u"Selection"_s][u"ForegroundNormal"_s];
    look.accentText = sets[u"View"_s][u"ForegroundLink"_s];
    look.accentWarning = contrast(look.accent, look.window) < 1.6;
    if (const QColor line = file.colors.value(u"line"_s); line.isValid())
        look.border = line;

    // Under the Plasma-based presets the font is Plasma's, family and size.
    const bool plasmaPreset = s.preset == u"plasma"_s || s.preset == u"glass"_s;
    look.systemFont = plasmaPreset && s.font == u"system"_s && !file.family && !file.sizePt;
    look.families = file.family ? (file.family->isEmpty() ? QStringList() : QStringList{*file.family}) : fontCandidates(s.font);
    look.fontSize = file.sizePt.value_or(s.fontSize);

    look.density = file.density.value_or(s.density) == u"compact"_s ? 0.72 : 1;
    look.radius = file.radius.value_or(s.radius);
    look.opacity = file.opacity.value_or(1 - s.translucency / 100.0);
    look.blur = file.blur.value_or(s.blur);
    look.popupSize = file.width || file.height ? u"custom"_s : s.popupSize;
    look.screenPercent = s.screenPercent;
    look.popupWidth = file.width.value_or(s.popupWidth);
    look.popupHeight = file.height.value_or(s.popupHeight);
    return look;
}

PopupSize popupSize(const Look &look, QSize screen)
{
    PopupSize result;
    QSize &wanted = result.wanted;
    if (look.popupSize == u"small"_s) {
        wanted = {940, 620};
    } else if (look.popupSize == u"screen"_s) {
        // A share of the screen's width, in the default's proportions, at most 90 % high.
        int w = qBound(640, int(std::lround(screen.width() * look.screenPercent / 100.0)), 1400);
        int h = int(std::lround(w * 640 / 1100.0));
        const int maxH = int(std::lround(screen.height() * 0.9));
        if (h > maxH) {
            h = maxH;
            w = int(std::lround(h * 1100 / 640.0));
        }
        wanted = {w, std::max(h, 400)};
    } else if (look.popupSize == u"custom"_s) {
        wanted = {qBound(480, look.popupWidth, 1600), qBound(320, look.popupHeight, 1000)};
    } else {
        wanted = {1100, 640};
    }
    // Room for a panel above or below (60 px each) and 16 px at the sides.
    const QSize fit(std::max(screen.width() - 32, 320), std::max(screen.height() - 120, 240));
    result.size = wanted.boundedTo(fit);
    result.clamped = result.size != wanted;
    return result;
}

QString schemeText(const Look &look)
{
    QString out;
    // Breeze's state effects: disabled text fades, inactive windows keep their colours.
    out += u"[ColorEffects:Disabled]\nColor=56,56,56\nColorAmount=0\nColorEffect=0\nContrastAmount=0.65\n"
           "ContrastEffect=1\nIntensityAmount=0.1\nIntensityEffect=2\n\n"
           "[ColorEffects:Inactive]\nChangeSelectionColor=true\nColor=112,111,110\nColorAmount=0.025\n"
           "ColorEffect=2\nContrastAmount=0.1\nContrastEffect=2\nEnable=false\nIntensityAmount=0\n"
           "IntensityEffect=0\n\n"_s;
    auto rgb = [](const QColor &c) {
        return u"%1,%2,%3"_s.arg(c.red()).arg(c.green()).arg(c.blue());
    };
    for (auto set = look.sets.cbegin(); set != look.sets.cend(); ++set) {
        out += u"[Colors:%1]\n"_s.arg(set.key());
        for (auto role = set->cbegin(); role != set->cend(); ++role)
            out += u"%1=%2\n"_s.arg(role.key(), rgb(role.value()));
        out += u'\n';
    }
    const ColorSet header = look.sets.value(u"Header"_s);
    out += u"[General]\nColorScheme=Endex\nName=Endex\n\n[WM]\n"_s;
    out += u"activeBackground=%1\nactiveBlend=%1\nactiveForeground=%2\n"_s.arg(rgb(header.value(u"BackgroundNormal"_s)),
                                                                             rgb(header.value(u"ForegroundNormal"_s)));
    out += u"inactiveBackground=%1\ninactiveBlend=%1\ninactiveForeground=%2\n"_s.arg(rgb(header.value(u"BackgroundAlternate"_s)),
                                                                                   rgb(header.value(u"ForegroundInactive"_s)));
    return out;
}

QString sampleToml(const Look &look, QSize popup)
{
    QString family = look.families.value(0);
    family.replace(u'\\', u"\\\\"_s).replace(u'"', u"\\\""_s);
    QString out = QObject::tr("# Endex 主题文件：没写的项沿用「外观」页的设置，写了的项优先。保存后立即生效。\n");
    out += u"[theme]\nbase = \"%1\"            # plasma | proto\nscheme = \"%2\"          # system | light | dark\n\n"_s.arg(
        look.base, look.schemeSetting);
    const QString scheme = look.scheme == u"dark"_s ? QObject::tr("深色") : QObject::tr("浅色");
    out += QObject::tr("[colors]                  # 按「%1」展开\n").arg(scheme);
    const std::pair<const char *, QColor> colors[] = {
        {"accent", look.accent},
        {"window", look.window},
        {"view", look.view},
        {"text", look.text},
        {"muted", look.muted},
        {"line", look.border.isValid() ? look.border : mix(look.window, look.text, 0.25)},
        {"positive", look.positive},
        {"neutral", look.neutral},
        {"negative", look.negative},
    };
    for (const auto &[key, color] : colors)
        out += u"%1 = \"%2\"\n"_s.arg(QLatin1StringView(key), hex(color));
    const QString system = family.isEmpty() ? QObject::tr("               # 空 = 系统字体") : QString();
    out += u"\n[font]\nfamily = \"%1\"%2\nsize_pt = %3\n\n"_s.arg(family, system).arg(look.fontSize, 0, 'f', 1);
    out += u"[shape]\nradius = %1\ndensity = \"%2\"     # comfortable | compact\n\n"_s.arg(look.radius).arg(
        look.density < 1 ? u"compact"_s : u"comfortable"_s);
    out += u"[popup]\nwidth = %1\nheight = %2\nopacity = %3\nblur = %4"_s.arg(popup.width())
               .arg(popup.height())
               .arg(look.opacity, 0, 'f', 2)
               .arg(look.blur ? u"true"_s : u"false"_s);
    out += QObject::tr("              # 模糊强度在 KWin 的「模糊」特效里调\n");
    return out;
}

}
