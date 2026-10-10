#include "results.h"

#include <QDBusArgument>
#include <QDateTime>
#include <QDir>
#include <QFileInfo>
#include <QLocale>
#include <QMimeDatabase>
#include <QPoint>
#include <QRegularExpression>
#include <QUrl>

using namespace Qt::StringLiterals;

static QList<QPair<uint, uint>> uintPairs(const QVariant &value)
{
    QList<QPair<uint, uint>> pairs;
    if (!value.canConvert<QDBusArgument>())
        return pairs;
    const auto arg = value.value<QDBusArgument>();
    arg.beginArray();
    while (!arg.atEnd()) {
        uint start = 0, length = 0;
        arg.beginStructure();
        arg >> start >> length;
        arg.endStructure();
        pairs.append({start, length});
    }
    arg.endArray();
    return pairs;
}

QList<QPair<int, int>> utf16Spans(const QString &text, const QVariant &codePointSpans)
{
    // offsets[i] = UTF-16 index of code point i.
    QList<int> offsets;
    for (int i = 0; i < text.size(); ++i) {
        offsets.append(i);
        if (text.at(i).isHighSurrogate() && i + 1 < text.size())
            ++i;
    }
    offsets.append(text.size());
    QList<QPair<int, int>> spans;
    for (const auto &[start, length] : uintPairs(codePointSpans)) {
        const qsizetype end = qsizetype(start) + length;
        if (end < offsets.size())
            spans.append({offsets[start], offsets[end] - offsets[start]});
    }
    return spans;
}

Hit Hit::fromMap(const QVariantMap &m)
{
    Hit h;
    h.id = m.value(u"id"_s).toString();
    h.kind = m.value(u"kind"_s).toString();
    h.path = m.value(u"path"_s).toString();
    h.title = m.value(u"title"_s).toString();
    h.location = m.value(u"location"_s).toString();
    h.snippet = m.value(u"snippet"_s).toString();
    h.spans = utf16Spans(h.snippet, m.value(u"highlights"_s));
    h.mtime = m.value(u"mtime"_s).toLongLong();
    h.size = m.value(u"size"_s).toULongLong();
    h.score = m.value(u"score"_s).toDouble();
    h.vecScore = m.value(u"vec_score"_s, -1.0).toDouble();
    h.kwScore = m.value(u"kw_score"_s, -1.0).toDouble();
    h.kwRank = m.value(u"kw_rank"_s).toInt();
    h.ocr = m.value(u"ocr"_s).toBool();
    h.width = m.value(u"width"_s).toInt();
    h.height = m.value(u"height"_s).toInt();
    return h;
}

QVariantMap Hit::toMap() const
{
    const QFileInfo info(path);
    return {
        {u"id"_s, id},
        {u"kind"_s, kind},
        {u"path"_s, path},
        {u"url"_s, QUrl::fromLocalFile(path)},
        {u"title"_s, title},
        {u"name"_s, info.fileName()},
        {u"dir"_s, homeRelative(info.path())},
        {u"location"_s, localizedLocation(location)},
        {u"snippet"_s, snippet},
        {u"when"_s, relativeTime(mtime)},
        {u"size"_s, QLocale().formattedDataSize(qint64(size), 1, QLocale::DataSizeTraditionalFormat)},
        {u"icon"_s, QMimeDatabase().mimeTypeForFile(path, QMimeDatabase::MatchExtension).iconName()},
        {u"ocr"_s, ocr},
        {u"width"_s, width},
        {u"height"_s, height},
    };
}

QString localizedLocation(const QString &location)
{
    static const QRegularExpression re(uR"(^(?:(.*) · )?(lines|paragraphs) (\d+)(?:-(\d+))?$)"_s);
    const auto m = re.match(location);
    if (!m.hasMatch())
        return location;
    QString heading = m.captured(1);
    heading.replace(u" > "_s, u" › "_s);
    const QString range = m.captured(4).isEmpty() ? m.captured(3) : m.captured(3) + u'–' + m.captured(4);
    const QString place = m.captured(2) == u"lines"_s ? QObject::tr("第 %1 行").arg(range) : QObject::tr("第 %1 段").arg(range);
    return heading.isEmpty() ? place : heading + u" · "_s + place;
}

QString relativeTime(qint64 unixSeconds)
{
    if (unixSeconds <= 0)
        return {};
    const QDateTime when = QDateTime::fromSecsSinceEpoch(unixSeconds);
    const QDate today = QDate::currentDate();
    const qint64 days = when.date().daysTo(today);
    if (days == 0)
        return QObject::tr("今天 %1").arg(when.toString(u"HH:mm"_s));
    if (days == 1)
        return QObject::tr("昨天");
    if (days > 1 && days < 7)
        return QObject::tr("%1 天前").arg(days);
    if (when.date().year() == today.year())
        return QObject::tr("%1 月 %2 日").arg(when.date().month()).arg(when.date().day());
    return QObject::tr("%1 年 %2 月 %3 日").arg(when.date().year()).arg(when.date().month()).arg(when.date().day());
}

QString homeRelative(const QString &path)
{
    const QString home = QDir::homePath();
    if (path == home)
        return u"~"_s;
    if (path.startsWith(home + u'/'))
        return u"~"_s + path.mid(home.size());
    return path;
}

int HitModel::rowCount(const QModelIndex &parent) const
{
    return parent.isValid() ? 0 : int(m_hits.size());
}

QVariant HitModel::data(const QModelIndex &index, int role) const
{
    if (!checkIndex(index, CheckIndexOption::IndexIsValid))
        return {};
    const Hit &h = m_hits.at(index.row());
    switch (role) {
    case UrlRole:
        return QUrl::fromLocalFile(h.path);
    case TitleRole:
        return h.title;
    case NameRole:
        return QFileInfo(h.path).fileName();
    case DirRole:
        return homeRelative(QFileInfo(h.path).path());
    case LocationRole:
        return localizedLocation(h.location);
    case SnippetRole:
        return h.snippet;
    case SpansRole: {
        QVariantList spans;
        for (const auto &[start, length] : h.spans)
            spans.append(QVariant::fromValue(QPoint(start, length)));
        return spans;
    }
    case WhenRole:
        return relativeTime(h.mtime);
    case IconRole:
        return QMimeDatabase().mimeTypeForFile(h.path, QMimeDatabase::MatchExtension).iconName();
    case VecScoreRole:
        return h.vecScore;
    case KwScoreRole:
        return h.kwScore;
    case KwRankRole:
        return h.kwRank;
    case OcrRole:
        return h.ocr;
    }
    return {};
}

QHash<int, QByteArray> HitModel::roleNames() const
{
    return {
        {UrlRole, "url"},
        {TitleRole, "title"},
        {NameRole, "name"},
        {DirRole, "dir"},
        {LocationRole, "location"},
        {SnippetRole, "snippet"},
        {SpansRole, "spans"},
        {WhenRole, "when"},
        {IconRole, "icon"},
        {VecScoreRole, "vecScore"},
        {KwScoreRole, "kwScore"},
        {KwRankRole, "kwRank"},
        {OcrRole, "ocr"},
    };
}

void HitModel::setHits(QList<Hit> hits)
{
    const bool resized = hits.size() != m_hits.size();
    beginResetModel();
    m_hits = std::move(hits);
    endResetModel();
    if (resized)
        Q_EMIT countChanged();
}

Results::Results(QObject *parent)
    : QObject(parent)
{
    m_nav.groups[Navigation::Docs] = {.pageSize = DocsPerPage, .columns = 1};
    m_nav.groups[Navigation::Images] = {.pageSize = ImagesPerPage, .columns = ImageColumns};
}

void Results::set(const QString &query, const QString &mode, QList<Hit> docs, QList<Hit> images)
{
    m_query = query;
    m_mode = mode;
    m_docs = std::move(docs);
    m_images = std::move(images);
    auto &[d, i] = m_nav.groups;
    d.count = int(m_docs.size());
    i.count = int(m_images.size());
    d.page = d.row = i.page = i.row = 0;
    m_nav.current = d.count ? Navigation::Docs : i.count ? Navigation::Images : Navigation::None;
    m_shownPages[0] = m_shownPages[1] = -1;
    setNavigating(false);
    showPages();
    Q_EMIT resultsChanged();
    Q_EMIT selectionChanged();
}

void Results::clear()
{
    set({}, m_mode, {}, {});
}

int Results::row() const
{
    return m_nav.current == Navigation::None ? -1 : m_nav.groups[m_nav.current].row;
}

const Hit *Results::currentHit() const
{
    if (m_nav.current == Navigation::None)
        return nullptr;
    const auto &g = m_nav.groups[m_nav.current];
    const auto &hits = m_nav.current == Navigation::Docs ? m_docs : m_images;
    const qsizetype i = qsizetype(g.page) * g.pageSize + g.row;
    return i < hits.size() ? &hits[i] : nullptr;
}

QVariantMap Results::current() const
{
    const Hit *h = currentHit();
    return h ? h->toMap() : QVariantMap{};
}

void Results::showPages()
{
    const QList<Hit> *hits[2] = {&m_docs, &m_images};
    HitModel *models[2] = {&m_docModel, &m_imageModel};
    for (int g = 0; g < 2; ++g) {
        const auto &group = m_nav.groups[g];
        if (m_shownPages[g] == group.page)
            continue;
        m_shownPages[g] = group.page;
        models[g]->setHits(hits[g]->mid(qsizetype(group.page) * group.pageSize, group.pageSize));
    }
}

bool Results::key(int qtKey)
{
    using Navigation::Key;
    static const QHash<int, Key> keys = {
        {Qt::Key_Up, Key::Up},
        {Qt::Key_Down, Key::Down},
        {Qt::Key_Left, Key::Left},
        {Qt::Key_Right, Key::Right},
        {Qt::Key_PageUp, Key::PageUp},
        {Qt::Key_PageDown, Key::PageDown},
        {Qt::Key_Tab, Key::SwitchGroup},
        {Qt::Key_Backtab, Key::SwitchGroup},
    };
    const auto it = keys.find(qtKey);
    if (it == keys.end())
        return false;
    const bool sideways = *it == Key::Left || *it == Key::Right;
    if ((sideways && !m_navigating) || !Navigation::move(m_nav, *it))
        return false;
    setNavigating(true);
    showPages();
    Q_EMIT selectionChanged();
    return true;
}

void Results::select(int group, int row)
{
    if (group < 0 || group > 1 || row < 0 || row >= m_nav.groups[group].rowsOn(m_nav.groups[group].page))
        return;
    m_nav.current = group;
    m_nav.groups[group].row = row;
    setNavigating(true);
    Q_EMIT selectionChanged();
}

void Results::turnPage(int group, int delta)
{
    if (group < 0 || group > 1 || m_nav.groups[group].count == 0)
        return;
    m_nav.current = group;
    Navigation::move(m_nav, delta > 0 ? Navigation::Key::PageDown : Navigation::Key::PageUp);
    setNavigating(true);
    showPages();
    Q_EMIT selectionChanged();
}

void Results::releaseArrows()
{
    setNavigating(false);
}

void Results::setNavigating(bool navigating)
{
    if (navigating == m_navigating)
        return;
    m_navigating = navigating;
    Q_EMIT arrowsInGridChanged();
}

QString Results::markup(const QString &text, const QVariantList &spans, const QColor &background, const QColor &foreground) const
{
    const QString open = u"<span style=\"background-color:%1;color:%2\">"_s.arg(background.name(), foreground.name());
    QString out;
    int at = 0;
    for (const QVariant &span : spans) {
        const QPoint p = span.toPoint();
        if (p.x() < at || p.x() + p.y() > text.size())
            continue;
        out += text.mid(at, p.x() - at).toHtmlEscaped() + open + text.mid(p.x(), p.y()).toHtmlEscaped() + u"</span>"_s;
        at = p.x() + p.y();
    }
    out += text.mid(at).toHtmlEscaped();
    out.replace(u'\n', u"<br>"_s);
    return out;
}
