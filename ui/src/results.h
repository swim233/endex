// Search results as two paged groups (documents, images) with one keyboard selection.

#pragma once

#include <QAbstractListModel>
#include <QColor>
#include <QList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

#include "navigation.h"

struct Hit {
    QString id, kind, path, title, location, snippet;
    // Highlighted (start, length) ranges of `snippet`, in UTF-16 units.
    QList<QPair<int, int>> spans;
    qint64 mtime = 0;
    quint64 size = 0;
    double score = 0;
    // Negative when that side did not rank the file.
    double vecScore = -1, kwScore = -1;
    int kwRank = 0;
    bool ocr = false;
    int width = 0, height = 0;

    static Hit fromMap(const QVariantMap &map);
    QVariantMap toMap() const;
};

// Daemon ranges count Unicode characters; QString indexes UTF-16 units.
QList<QPair<int, int>> utf16Spans(const QString &text, const QVariant &codePointSpans);
// `Heading > Sub · lines 4-5` → `Heading › Sub · 第 4–5 行`.
QString localizedLocation(const QString &location);
QString relativeTime(qint64 unixSeconds);
QString homeRelative(const QString &path);

class HitModel : public QAbstractListModel
{
    Q_OBJECT
    QML_ELEMENT
    QML_UNCREATABLE("Owned by Results")
    Q_PROPERTY(int count READ count NOTIFY countChanged)

public:
    enum Role {
        UrlRole = Qt::UserRole + 1,
        TitleRole,
        NameRole,
        DirRole,
        LocationRole,
        SnippetRole,
        SpansRole,
        WhenRole,
        IconRole,
        VecScoreRole,
        KwScoreRole,
        KwRankRole,
        OcrRole,
    };

    using QAbstractListModel::QAbstractListModel;

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    int count() const { return int(m_hits.size()); }
    void setHits(QList<Hit> hits);

Q_SIGNALS:
    void countChanged();

private:
    QList<Hit> m_hits;
};

class Results : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_UNCREATABLE("Created by main")
    Q_PROPERTY(HitModel *docs READ docs CONSTANT)
    Q_PROPERTY(HitModel *images READ images CONSTANT)
    Q_PROPERTY(QString query READ query NOTIFY resultsChanged)
    Q_PROPERTY(QString mode READ mode NOTIFY resultsChanged)
    Q_PROPERTY(int docCount READ docCount NOTIFY resultsChanged)
    Q_PROPERTY(int imageCount READ imageCount NOTIFY resultsChanged)
    Q_PROPERTY(int docPage READ docPage NOTIFY selectionChanged)
    Q_PROPERTY(int docPages READ docPages NOTIFY resultsChanged)
    Q_PROPERTY(int imagePage READ imagePage NOTIFY selectionChanged)
    Q_PROPERTY(int imagePages READ imagePages NOTIFY resultsChanged)
    // Navigation::Docs, Navigation::Images or Navigation::None.
    Q_PROPERTY(int group READ group NOTIFY selectionChanged)
    // The selected item's index on its group's page.
    Q_PROPERTY(int row READ row NOTIFY selectionChanged)
    Q_PROPERTY(QVariantMap current READ current NOTIFY selectionChanged)
    // ←/→ move the text cursor until the user moves through the results (↑↓, PgUp/PgDn, Tab) or
    // clicks one; from then on, while an image is selected, they move in the grid. Typing or new
    // results hand them back to the search field.
    Q_PROPERTY(bool arrowsInGrid READ arrowsInGrid NOTIFY arrowsInGridChanged)

public:
    static constexpr int DocsPerPage = 8;
    static constexpr int ImagesPerPage = 9;
    static constexpr int ImageColumns = 3;

    explicit Results(QObject *parent = nullptr);

    HitModel *docs() { return &m_docModel; }
    HitModel *images() { return &m_imageModel; }
    QString query() const { return m_query; }
    QString mode() const { return m_mode; }
    int docCount() const { return int(m_docs.size()); }
    int imageCount() const { return int(m_images.size()); }
    int docPage() const { return m_nav.groups[Navigation::Docs].page; }
    int docPages() const { return m_nav.groups[Navigation::Docs].pages(); }
    int imagePage() const { return m_nav.groups[Navigation::Images].page; }
    int imagePages() const { return m_nav.groups[Navigation::Images].pages(); }
    int group() const { return m_nav.current; }
    int row() const;
    QVariantMap current() const;
    bool arrowsInGrid() const { return m_navigating; }

    void set(const QString &query, const QString &mode, QList<Hit> docs, QList<Hit> images);
    Q_INVOKABLE void clear();

    // Handles ↑ ↓ ← → PgUp PgDn Tab; false when the key is not for the result list.
    Q_INVOKABLE bool key(int qtKey);
    Q_INVOKABLE void select(int group, int row);
    Q_INVOKABLE void turnPage(int group, int delta);
    Q_INVOKABLE void releaseArrows();
    // Escaped rich text with `spans` (as from the `spans` role) highlighted.
    Q_INVOKABLE QString markup(const QString &text, const QVariantList &spans, const QColor &background,
                               const QColor &foreground) const;

Q_SIGNALS:
    void resultsChanged();
    void selectionChanged();
    void arrowsInGridChanged();

private:
    const Hit *currentHit() const;
    void setNavigating(bool navigating);
    void showPages();

    QString m_query, m_mode;
    QList<Hit> m_docs, m_images;
    HitModel m_docModel, m_imageModel;
    Navigation::State m_nav;
    int m_shownPages[2] = {-1, -1};
    bool m_navigating = false;
};
