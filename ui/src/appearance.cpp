#include "appearance.h"

#include <QApplication>
#include <QCryptographicHash>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QFontDatabase>
#include <QJSEngine>
#include <QQuickItem>
#include <QQuickWindow>
#include <QScreen>
#include <QStandardPaths>

#include <KColorScheme>
#include <KIO/CommandLauncherJob>
#include <KSharedConfig>
#include <KWaylandExtras>
#include <KWindowEffects>

#include "settings.h"

using namespace Qt::StringLiterals;

static Appearance *s_instance = nullptr;

static constexpr qint64 ThemeFileLimit = 1 << 20;

Appearance::Appearance(Settings *settings, QObject *parent)
    : QObject(parent)
    , m_settings(settings)
{
    s_instance = this;
    m_globals = KConfigWatcher::create(KSharedConfig::openConfig(u"kdeglobals"_s));
    connect(m_globals.data(), &KConfigWatcher::configChanged, this, [this](const KConfigGroup &group) {
        if (group.name().startsWith(u"Colors:"_s) || group.name() == u"General"_s) {
            readSystem();
            update();
        }
    });
    // Editors save in several steps, and some replace the file.
    m_reread.setSingleShot(true);
    m_reread.setInterval(200);
    connect(&m_reread, &QTimer::timeout, this, [this] {
        readThemeFile();
        update();
    });
    connect(&m_watcher, &QFileSystemWatcher::fileChanged, &m_reread, qOverload<>(&QTimer::start));
    connect(&m_watcher, &QFileSystemWatcher::directoryChanged, &m_reread, qOverload<>(&QTimer::start));
    // Settings::changed also fires for draft edits; only the applied settings matter here.
    connect(m_settings, &Settings::changed, this, [this] {
        if (m_settings->saved() != m_saved)
            update();
    });
    readSystem();
    readThemeFile();
    m_blurAvailable = KWindowEffects::isEffectAvailable(KWindowEffects::BlurBehind);
    update();
}

Appearance *Appearance::create(QQmlEngine *, QJSEngine *)
{
    QJSEngine::setObjectOwnership(s_instance, QJSEngine::CppOwnership);
    return s_instance;
}

QString Appearance::themePath() const
{
    return QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation) + u"/endex/theme.toml"_s;
}

// Plasma's small size in proportion: 8 pt next to 10 pt.
QFont Appearance::smallFont() const
{
    const QFont small = QFontDatabase::systemFont(QFontDatabase::SmallestReadableFont);
    if (!m_fontSet)
        return small;
    QFont font = QApplication::font();
    font.setPointSizeF(m_look.fontSize * small.pointSizeF() / QFontDatabase::systemFont(QFontDatabase::GeneralFont).pointSizeF());
    return font;
}

QSize Appearance::popupSize(QScreen *screen) const
{
    return look::popupSize(m_look, screen->size()).size;
}

look::Look Appearance::lookOf(const QVariantMap &settings) const
{
    return look::resolve(look::Settings::fromMap(settings), m_file, m_system, m_systemDark);
}

QStringList Appearance::installed(const QStringList &families) const
{
    QStringList found;
    for (const QString &family : families) {
        if (QFontDatabase::hasFamily(family))
            found.append(family);
    }
    return found;
}

QVariantMap Appearance::preview(const QVariantMap &draft, QQuickItem *item) const
{
    const look::Look l = lookOf(draft);
    QScreen *screen = item && item->window() ? item->window()->screen() : QGuiApplication::primaryScreen();
    const look::PopupSize size = look::popupSize(l, screen->size());
    const QFont system = QFontDatabase::systemFont(QFontDatabase::GeneralFont);
    const QStringList families = installed(l.families);
    return {
        {u"scheme"_s, l.scheme},
        {u"window"_s, l.window},
        {u"view"_s, l.view},
        {u"text"_s, l.text},
        {u"muted"_s, l.muted},
        {u"accent"_s, l.accent},
        {u"accentText"_s, l.accentText},
        {u"positive"_s, l.positive},
        {u"border"_s, l.border.isValid() ? l.border : look::mix(l.window, l.text, 0.25)},
        {u"accentWarning"_s, l.accentWarning},
        {u"family"_s, families.value(0, system.family())},
        // A chosen font that is not installed: its name.
        {u"missingFont"_s, !l.families.isEmpty() && families.isEmpty() ? l.families.first() : QString()},
        {u"fontSize"_s, l.systemFont ? system.pointSizeF() : l.fontSize},
        {u"density"_s, l.density},
        {u"radius"_s, l.radius},
        {u"opacity"_s, l.opacity},
        {u"width"_s, size.size.width()},
        {u"height"_s, size.size.height()},
        {u"wantedWidth"_s, size.wanted.width()},
        {u"wantedHeight"_s, size.wanted.height()},
        {u"clamped"_s, size.clamped},
    };
}

QVariantMap Appearance::presetValues(const QString &preset) const
{
    return look::presetValues(preset);
}

QString Appearance::installedFamily(const QString &font) const
{
    return installed(look::fontCandidates(font)).value(0);
}

QString Appearance::sampleToml(const QVariantMap &draft, QQuickItem *item) const
{
    look::Look l = lookOf(draft);
    // The family in use, so that the sample gives the same look; the chosen one if none is.
    if (const QStringList found = installed(l.families); !found.isEmpty())
        l.families = found;
    QScreen *screen = item && item->window() ? item->window()->screen() : QGuiApplication::primaryScreen();
    return look::sampleToml(l, look::popupSize(l, screen->size()).wanted);
}

void Appearance::checkBlur()
{
    const bool available = KWindowEffects::isEffectAvailable(KWindowEffects::BlurBehind);
    if (available == m_blurAvailable)
        return;
    m_blurAvailable = available;
    Q_EMIT blurAvailableChanged();
}

void Appearance::openEffectSettings(QQuickItem *from)
{
    auto *job = new KIO::CommandLauncherJob(u"systemsettings"_s, {u"kcm_kwin_effects"_s});
    job->setDesktopName(u"systemsettings"_s);
    connect(job, &KJob::result, this, [this, job] {
        if (job->error())
            Q_EMIT effectSettingsFailed(job->errorString());
    });
    QWindow *window = from ? from->window() : nullptr;
    if (!window) {
        job->start();
        return;
    }
    KWaylandExtras::xdgActivationToken(window, u"endex-ui"_s).then(this, [job](const QString &token) {
        job->setStartupId(token.toUtf8());
        job->start();
    });
}

void Appearance::update()
{
    m_saved = m_settings->saved();
    m_look = lookOf(m_saved);
    applyColors();
    applyFont();
    Q_EMIT changed();
}

// Plasma's colour sets as kdeglobals has them, whatever this process uses.
void Appearance::readSystem()
{
    static const std::pair<const char *, KColorScheme::ColorSet> Sets[] = {
        {"View", KColorScheme::View},
        {"Window", KColorScheme::Window},
        {"Button", KColorScheme::Button},
        {"Selection", KColorScheme::Selection},
        {"Tooltip", KColorScheme::Tooltip},
        {"Complementary", KColorScheme::Complementary},
        {"Header", KColorScheme::Header},
    };
    static const std::pair<const char *, KColorScheme::BackgroundRole> Backgrounds[] = {
        {"BackgroundNormal", KColorScheme::NormalBackground},
        {"BackgroundAlternate", KColorScheme::AlternateBackground},
    };
    static const std::pair<const char *, KColorScheme::ForegroundRole> Foregrounds[] = {
        {"ForegroundNormal", KColorScheme::NormalText},
        {"ForegroundInactive", KColorScheme::InactiveText},
        {"ForegroundActive", KColorScheme::ActiveText},
        {"ForegroundLink", KColorScheme::LinkText},
        {"ForegroundVisited", KColorScheme::VisitedText},
        {"ForegroundNegative", KColorScheme::NegativeText},
        {"ForegroundNeutral", KColorScheme::NeutralText},
        {"ForegroundPositive", KColorScheme::PositiveText},
    };
    const KSharedConfigPtr globals = m_globals->config();
    m_system.clear();
    for (const auto &[name, set] : Sets) {
        const KColorScheme scheme(QPalette::Active, set, globals);
        look::ColorSet &colors = m_system[QLatin1StringView(name)];
        for (const auto &[key, role] : Backgrounds)
            colors.insert(QLatin1StringView(key), scheme.background(role).color());
        for (const auto &[key, role] : Foregrounds)
            colors.insert(QLatin1StringView(key), scheme.foreground(role).color());
        colors.insert(u"DecorationFocus"_s, scheme.decoration(KColorScheme::FocusColor).color());
        colors.insert(u"DecorationHover"_s, scheme.decoration(KColorScheme::HoverColor).color());
    }
    const QColor window = m_system[u"Window"_s][u"BackgroundNormal"_s];
    m_systemDark = look::contrast(window, Qt::white) > look::contrast(window, Qt::black);
}

// A file that does not load keeps the last good theme; its error is shown on the page.
void Appearance::readThemeFile()
{
    watchThemeFile();
    const QFileInfo info(themePath());
    QFile file(info.filePath());
    QString error;
    // Read on the GUI thread: never a pipe or a device, which could block, nor a huge file.
    if (!info.exists()) {
        m_file = {};
    } else if (!info.isFile()) {
        error = tr("不是普通文件");
    } else if (info.size() > ThemeFileLimit) {
        error = tr("文件太大（超过 1 MiB）");
    } else if (!file.open(QIODevice::ReadOnly)) {
        error = file.errorString();
    } else if (auto parsed = look::ThemeFile::parse(QString::fromUtf8(file.readAll()), &error)) {
        m_file = *parsed;
    }
    m_themeError = error;
    Q_EMIT themeChanged();
}

// The folder too: the file may not exist yet, and editors replace it.
void Appearance::watchThemeFile()
{
    const QString path = themePath();
    const QString dir = QFileInfo(path).path();
    // Until the folder exists, its parent says when it appears.
    const QString parent = QFileInfo(dir).path();
    const QString watched = QFileInfo::exists(dir) ? dir : parent;
    if (watched == dir && m_watcher.directories().contains(parent))
        m_watcher.removePath(parent);
    if (!m_watcher.directories().contains(watched))
        m_watcher.addPath(watched);
    if (QFileInfo::exists(path) && !m_watcher.files().contains(path))
        m_watcher.addPath(path);
}

// The look's colours become this process's colour scheme, as KColorSchemeManager would apply a
// scheme chosen in an app: Kirigami, the Plasma controls and the window decoration follow it.
// The file is named after its content, so that no cached copy of an older one is used.
void Appearance::applyColors()
{
    const QString dir = QStandardPaths::writableLocation(QStandardPaths::CacheLocation);
    QString path;
    if (!m_look.systemColors) {
        const QByteArray text = look::schemeText(m_look).toUtf8();
        const QString hash = QString::fromLatin1(QCryptographicHash::hash(text, QCryptographicHash::Sha1).toHex().left(12));
        path = dir + u"/scheme-"_s + hash + u".colors"_s;
        QFile file(path);
        if (!file.exists() && (!QDir().mkpath(dir) || !file.open(QIODevice::WriteOnly) || file.write(text) != text.size())) {
            qWarning().noquote() << "endex-ui: cannot write the colour scheme" << path << file.errorString();
            return;
        }
    }
    if (path == m_schemePath)
        return;
    // Set before the palette: the platform integration reads it on the palette change.
    qApp->setProperty("KDE_COLOR_SCHEME_PATH", path);
    QApplication::setPalette(path.isEmpty() ? QPalette() : KColorScheme::createApplicationPalette(KSharedConfig::openConfig(path)));
    m_schemePath = path;
    for (const QString &old : QDir(dir).entryList({u"scheme-*.colors"_s}, QDir::Files)) {
        if (dir + u'/' + old != path)
            QFile::remove(dir + u'/' + old);
    }
}

void Appearance::applyFont()
{
    QFont font = QFontDatabase::systemFont(QFontDatabase::GeneralFont);
    if (m_look.systemFont) {
        if (m_fontSet)
            QApplication::setFont(font);
        m_fontSet = false;
        return;
    }
    // Missing families fall back to the system one; CJK glyphs come from it too.
    QStringList families = installed(m_look.families);
    if (!families.isEmpty())
        font.setFamilies(families << font.family());
    font.setPointSizeF(m_look.fontSize);
    if (m_fontSet && QApplication::font() == font)
        return;
    QApplication::setFont(font);
    m_fontSet = true;
}
