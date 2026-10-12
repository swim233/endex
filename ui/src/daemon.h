// Client of the search daemon, org.endex.Daemon1. Replies to superseded queries are dropped.

#pragma once

#include <QDBusServiceWatcher>
#include <QElapsedTimer>
#include <QObject>
#include <QTimer>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

#include <functional>

class Results;

class QDBusPendingCallWatcher;

// D-Bus containers (QDBusArgument, QDBusVariant) as plain lists and maps, for QML.
QVariant fromDBus(const QVariant &value);
// An asynchronous call of org.endex.Daemon1; the watcher belongs to `owner`.
QDBusPendingCallWatcher *callDaemon(QObject *owner, const QString &method, const QVariantList &args);

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
    // What the indexer does (the IndexState property), live from its change signals.
    Q_PROPERTY(QVariantMap indexState READ indexState NOTIFY indexStateChanged)
    // Recent queries from the usage log, the newest first (see fetchRecent).
    Q_PROPERTY(QStringList recent READ recent NOTIFY recentChanged)
    // restart() runs until the daemon answers again.
    Q_PROPERTY(bool restarting READ restarting NOTIFY restartChanged)
    // Why the last restart failed; cleared by the next one.
    Q_PROPERTY(QString restartError READ restartError NOTIFY restartChanged)
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
        // The index cannot be read while another process replaces it; poll Status.
        Rebuilding,
    };
    Q_ENUM(State)

    explicit Daemon(Results *results, QObject *parent = nullptr);

    State state() const { return m_state; }
    QString error() const { return m_error; }
    QString searchError() const { return m_searchError; }
    QVariantMap status() const { return m_status; }
    QVariantMap indexState() const { return m_indexState; }
    QStringList recent() const { return m_recent; }
    bool restarting() const { return m_restarting; }
    QString restartError() const { return m_restartError; }
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
    // `systemctl --user restart endexd`, for changes that need a restart (restart_required).
    Q_INVOKABLE void restart();
    Q_INVOKABLE void fetchRecent();
    // Pause, Resume, ContinueOnBattery, Rescan or Rebuild; Status follows.
    Q_INVOKABLE void control(const QString &method);
    // Failed files again; all of them if `paths` is empty.
    Q_INVOKABLE void retry(const QStringList &paths);
    // One popup session for the usage log: query, mode, results, latency_ms, picked.
    Q_INVOKABLE void reportUsage(const QVariantMap &entry);

Q_SIGNALS:
    void stateChanged();
    void statusChanged();
    void searchingChanged();
    void searchErrorChanged();
    void latencyChanged();
    void previewChanged();
    void indexStateChanged();
    void recentChanged();
    void restartChanged();
    // A control call (pause, rebuild, …) failed.
    void actionFailed(const QString &message);
    // Usable again (after a rebuild, a restart, a repaired index) and no search answered since:
    // the shown results may be stale.
    void recovered();

private Q_SLOTS:
    void propertiesChanged(const QString &interface, const QVariantMap &changed, const QStringList &invalidated);

private:
    // `live`: from a change signal, not from a Status reply.
    void setIndexState(const QVariantMap &state, bool live);
    void setState(State state, const QString &error = {});
    // Runs `systemctl --user <action> endexd`; `done` gets an empty string on success, else why
    // it failed.
    void systemctl(const QString &action, std::function<void(const QString &)> done);
    void setRestart(bool restarting, const QString &error = {});
    void setSearching(bool searching);
    void setSearchError(const QString &error);
    // Moves to Down for errors that mean "no daemon"; returns whether it did.
    bool handleUnreachable(const class QDBusError &error);

    Results *m_results;
    QDBusServiceWatcher m_watcher;
    QTimer m_startTimeout;
    QTimer m_restartTimeout;
    State m_state = Connecting;
    QString m_error;
    QString m_searchError;
    QVariantMap m_status;
    QVariantMap m_indexState;
    QStringList m_recent;
    bool m_restarting = false;
    // systemctl returned; the next Status answer ends the restart.
    bool m_restarted = false;
    QString m_restartError;
    bool m_searching = false;
    // No search answered since the daemon was last not Ready.
    bool m_stale = true;
    int m_latency = 0;
    uint m_queryId = 0;
    QVariantMap m_preview;
    QString m_previewId;
};
