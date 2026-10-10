// Client of the search daemon, org.endex.Daemon1. Replies to superseded queries are dropped.

#pragma once

#include <QDBusServiceWatcher>
#include <QElapsedTimer>
#include <QObject>
#include <QTimer>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

class Results;

class Daemon : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_UNCREATABLE("Created by main")
    Q_PROPERTY(State state READ state NOTIFY stateChanged)
    // Why the daemon is down or the index unusable.
    Q_PROPERTY(QString error READ error NOTIFY stateChanged)
    // Why the last search failed; cleared by the next answered search. A Status refresh does
    // not clear it: the index can be "ready" while one search kind fails.
    Q_PROPERTY(QString searchError READ searchError NOTIFY searchErrorChanged)
    // The last Status() reply.
    Q_PROPERTY(QVariantMap status READ status NOTIFY statusChanged)
    Q_PROPERTY(bool searching READ searching NOTIFY searchingChanged)
    // Round trip of the last answered search, in milliseconds.
    Q_PROPERTY(int latency READ latency NOTIFY latencyChanged)
    // The last Preview() reply for `previewId`, with `spans` for highlighting.
    Q_PROPERTY(QVariantMap preview READ preview NOTIFY previewChanged)
    Q_PROPERTY(QString previewId READ previewId NOTIFY previewChanged)

public:
    enum State {
        Connecting,
        Ready,
        // Nothing indexed yet.
        Empty,
        // The daemon runs but cannot use the index (e.g. built with another model).
        Broken,
        Down,
        Starting,
        // Another process is replacing the index (`endex index --rebuild`); poll Status.
        Rebuilding,
    };
    Q_ENUM(State)

    explicit Daemon(Results *results, QObject *parent = nullptr);

    State state() const { return m_state; }
    QString error() const { return m_error; }
    QString searchError() const { return m_searchError; }
    QVariantMap status() const { return m_status; }
    bool searching() const { return m_searching; }
    int latency() const { return m_latency; }
    QVariantMap preview() const { return m_preview; }
    QString previewId() const { return m_previewId; }

    Q_INVOKABLE void refresh();
    // An empty query clears the results without asking the daemon.
    Q_INVOKABLE void search(const QString &query, const QString &mode);
    Q_INVOKABLE void fetchPreview(const QString &id, const QString &query);
    // Runs `systemctl --user start endexd` (ENDEX_SYSTEMCTL overrides the program, for tests).
    Q_INVOKABLE void start();

Q_SIGNALS:
    void stateChanged();
    void statusChanged();
    void searchingChanged();
    void searchErrorChanged();
    void latencyChanged();
    void previewChanged();
    // Usable again (after a rebuild, a restart, a repaired index) and no search answered since:
    // the shown results may be stale.
    void recovered();

private:
    void setState(State state, const QString &error = {});
    void setSearching(bool searching);
    void setSearchError(const QString &error);
    // Moves to Down for errors that mean "no daemon"; returns whether it did.
    bool handleUnreachable(const class QDBusError &error);

    Results *m_results;
    QDBusServiceWatcher m_watcher;
    QTimer m_startTimeout;
    State m_state = Connecting;
    QString m_error;
    QString m_searchError;
    QVariantMap m_status;
    bool m_searching = false;
    // No search answered since the daemon was last not Ready.
    bool m_stale = true;
    int m_latency = 0;
    uint m_queryId = 0;
    QVariantMap m_preview;
    QString m_previewId;
};
