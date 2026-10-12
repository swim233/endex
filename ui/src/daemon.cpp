#include "daemon.h"

#include <QDBusArgument>
#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QGuiApplication>
#include <QPoint>
#include <QWindow>
#include <QProcess>

#include "results.h"

using namespace Qt::StringLiterals;

static const auto Service = u"org.endex.Daemon1"_s;
static const auto Path = u"/org/endex/Daemon"_s;
static const auto Interface = u"org.endex.Daemon1"_s;
static constexpr uint LimitDocs = 64;
static constexpr uint LimitImages = 72;
// Long enough for D-Bus activation, which loads the embedding model.
static constexpr int CallTimeoutMs = 30000;

Daemon::Daemon(Results *results, QObject *parent)
    : QObject(parent)
    , m_results(results)
    , m_watcher(Service, QDBusConnection::sessionBus(), QDBusServiceWatcher::WatchForOwnerChange)
{
    connect(&m_watcher, &QDBusServiceWatcher::serviceRegistered, this, &Daemon::refresh);
    connect(&m_watcher, &QDBusServiceWatcher::serviceUnregistered, this, [this] {
        setState(Down, tr("守护进程已退出"));
    });
    m_startTimeout.setSingleShot(true);
    m_startTimeout.setInterval(20000);
    connect(&m_startTimeout, &QTimer::timeout, this, [this] {
        if (m_state == Starting)
            setState(Down, tr("启动超时。用 journalctl --user -u endexd 查看日志。"));
    });
    m_restartTimeout.setSingleShot(true);
    m_restartTimeout.setInterval(60000);
    connect(&m_restartTimeout, &QTimer::timeout, this, [this] {
        if (m_restarting)
            setRestart(false, tr("重启后一分钟内守护进程没有回应。用 journalctl --user -u endexd 查看日志。"));
    });
    QDBusConnection::sessionBus().connect(Service, Path, u"org.freedesktop.DBus.Properties"_s, u"PropertiesChanged"_s, this,
                                          SLOT(propertiesChanged(QString, QVariantMap, QStringList)));
}

QVariant fromDBus(const QVariant &value)
{
    if (value.metaType() == QMetaType::fromType<QDBusVariant>())
        return fromDBus(value.value<QDBusVariant>().variant());
    if (value.metaType() == QMetaType::fromType<QVariantMap>()) {
        QVariantMap map = value.toMap();
        for (auto it = map.begin(); it != map.end(); ++it)
            *it = fromDBus(*it);
        return map;
    }
    if (value.metaType() != QMetaType::fromType<QDBusArgument>())
        return value;
    const auto arg = value.value<QDBusArgument>();
    switch (arg.currentType()) {
    case QDBusArgument::MapType: {
        QVariantMap map;
        arg.beginMap();
        while (!arg.atEnd()) {
            arg.beginMapEntry();
            const QString key = arg.asVariant().toString();
            map.insert(key, fromDBus(arg.asVariant()));
            arg.endMapEntry();
        }
        arg.endMap();
        return map;
    }
    case QDBusArgument::ArrayType: {
        QVariantList list;
        arg.beginArray();
        while (!arg.atEnd())
            list.append(fromDBus(arg.asVariant()));
        arg.endArray();
        return list;
    }
    default:
        return fromDBus(arg.asVariant());
    }
}

QDBusPendingCallWatcher *callDaemon(QObject *owner, const QString &method, const QVariantList &args)
{
    auto message = QDBusMessage::createMethodCall(Service, Path, Interface, method);
    message.setArguments(args);
    auto pending = QDBusConnection::sessionBus().asyncCall(message, CallTimeoutMs);
    return new QDBusPendingCallWatcher(pending, owner);
}

void Daemon::refresh()
{
    connect(callDaemon(this, u"Status"_s, {}), &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *w) {
        w->deleteLater();
        const QDBusPendingReply<QVariantMap> reply = *w;
        if (reply.isError()) {
            if (!handleUnreachable(reply.error()))
                setState(Broken, reply.error().message());
            return;
        }
        m_status = fromDBus(reply.value()).toMap();
        Q_EMIT statusChanged();
        // The first answer after systemctl returned comes from the new daemon (Type=dbus).
        if (m_restarted)
            setRestart(false);
        setIndexState(m_status.value(u"index_state"_s).toMap(), false);
        const QString state = m_status.value(u"state"_s).toString();
        if (state == u"ready"_s)
            setState(Ready);
        else if (state == u"empty"_s)
            setState(Empty);
        else if (state == u"rebuilding"_s)
            setState(Rebuilding);
        else
            setState(Broken, m_status.value(u"error"_s).toString());
    });
}

void Daemon::search(const QString &query, const QString &mode)
{
    const uint id = ++m_queryId;
    if (query.trimmed().isEmpty()) {
        setSearching(false);
        setSearchError({});
        m_results->set({}, mode, {}, {});
        return;
    }
    setSearching(true);
    const QVariantMap opts = {
        {u"query_id"_s, id},
        {u"limit_docs"_s, LimitDocs},
        {u"limit_images"_s, LimitImages},
    };
    QElapsedTimer timer;
    timer.start();
    auto watcher = callDaemon(this, u"Search"_s, {query, mode, opts});
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [=, this](QDBusPendingCallWatcher *w) {
        w->deleteLater();
        if (id != m_queryId)
            return;
        setSearching(false);
        const QDBusPendingReply<uint, QList<QVariantMap>, QList<QVariantMap>> reply = *w;
        if (reply.isError()) {
            if (reply.error().name() != u"org.endex.Daemon1.Error.Cancelled"_s && !handleUnreachable(reply.error())) {
                // No results of an earlier query or mode stay on screen under this one.
                m_results->set(query, mode, {}, {});
                setSearchError(reply.error().message());
                // Status tells a rebuild in progress from a broken index.
                refresh();
            }
            return;
        }
        m_latency = int(timer.elapsed());
        Q_EMIT latencyChanged();
        m_stale = false;
        setSearchError({});
        if (m_state != Ready)
            refresh();
        QList<Hit> docs, images;
        for (const QVariantMap &m : reply.argumentAt<1>())
            docs.append(Hit::fromMap(m));
        for (const QVariantMap &m : reply.argumentAt<2>())
            images.append(Hit::fromMap(m));
        m_results->set(query, mode, std::move(docs), std::move(images));
    });
}

void Daemon::fetchPreview(const QString &id, const QString &query)
{
    m_previewId = id;
    m_preview.clear();
    Q_EMIT previewChanged();
    if (id.isEmpty())
        return;
    connect(callDaemon(this, u"Preview"_s, {id, query}), &QDBusPendingCallWatcher::finished, this, [=, this](QDBusPendingCallWatcher *w) {
        w->deleteLater();
        if (id != m_previewId)
            return;
        const QDBusPendingReply<QVariantMap> reply = *w;
        if (reply.isError()) {
            handleUnreachable(reply.error());
            return;
        }
        m_preview = reply.value();
        QVariantList spans;
        for (const auto &[start, length] : utf16Spans(m_preview.value(u"text"_s).toString(), m_preview.value(u"highlights"_s)))
            spans.append(QVariant::fromValue(QPoint(start, length)));
        m_preview.insert(u"spans"_s, spans);
        m_preview.remove(u"highlights"_s);
        m_preview.insert(u"location"_s, localizedLocation(m_preview.value(u"location"_s).toString()));
        Q_EMIT previewChanged();
    });
}

void Daemon::fetchRecent()
{
    connect(callDaemon(this, u"RecentQueries"_s, {8u}), &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *w) {
        w->deleteLater();
        const QDBusPendingReply<QStringList> reply = *w;
        const QStringList recent = reply.isError() ? QStringList() : reply.value();
        if (recent != m_recent) {
            m_recent = recent;
            Q_EMIT recentChanged();
        }
    });
}

void Daemon::control(const QString &method)
{
    static const QStringList methods = {u"Pause"_s, u"Resume"_s, u"ContinueOnBattery"_s, u"Rescan"_s, u"Rebuild"_s};
    Q_ASSERT(methods.contains(method));
    if (!methods.contains(method))
        return;
    connect(callDaemon(this, method, {}), &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *w) {
        w->deleteLater();
        const QDBusPendingReply<> reply = *w;
        if (reply.isError() && !handleUnreachable(reply.error()))
            Q_EMIT actionFailed(reply.error().message());
        refresh();
    });
}

void Daemon::retry(const QStringList &paths)
{
    connect(callDaemon(this, u"Retry"_s, {paths}), &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *w) {
        w->deleteLater();
        const QDBusPendingReply<> reply = *w;
        if (reply.isError() && !handleUnreachable(reply.error()))
            Q_EMIT actionFailed(reply.error().message());
    });
}

void Daemon::reportUsage(const QVariantMap &entry)
{
    const QVariantMap typed = {
        {u"query"_s, entry.value(u"query"_s).toString()},
        {u"mode"_s, entry.value(u"mode"_s).toString()},
        {u"results"_s, entry.value(u"results"_s).toUInt()},
        {u"latency_ms"_s, entry.value(u"latency_ms"_s).toUInt()},
        {u"picked"_s, entry.value(u"picked"_s).toString()},
    };
    // Fire and forget: a lost entry only shortens the recent searches.
    auto message = QDBusMessage::createMethodCall(Service, Path, Interface, u"ReportUsage"_s);
    message.setArguments({typed});
    QDBusConnection::sessionBus().asyncCall(message, CallTimeoutMs);
}

void Daemon::propertiesChanged(const QString &interface, const QVariantMap &changed, const QStringList &)
{
    if (interface == Interface && changed.contains(u"IndexState"_s))
        setIndexState(fromDBus(changed.value(u"IndexState"_s)).toMap(), true);
}

void Daemon::setIndexState(const QVariantMap &state, bool live)
{
    if (state == m_indexState)
        return;
    const bool phase = state.value(u"state"_s) != m_indexState.value(u"state"_s);
    m_indexState = state;
    Q_EMIT indexStateChanged();
    // Counts and per-folder numbers change with the phase (e.g. at the end of a run); only
    // worth asking for while a window shows them (a summon refreshes anyway).
    const auto windows = QGuiApplication::topLevelWindows();
    const bool shown = std::any_of(windows.cbegin(), windows.cend(), [](QWindow *w) {
        return w->isVisible();
    });
    if (live && phase && shown)
        refresh();
}

void Daemon::start()
{
    setState(Starting);
    m_startTimeout.start();
    systemctl(u"start"_s, [this](const QString &error) {
        if (error.isEmpty())
            refresh(); // Type=dbus: the name is already on the bus.
        else
            setState(Down, error);
    });
}

void Daemon::restart()
{
    if (m_restarting)
        return;
    setRestart(true);
    m_restartTimeout.start();
    systemctl(u"restart"_s, [this](const QString &error) {
        if (!error.isEmpty()) {
            setRestart(false, error);
            return;
        }
        m_restarted = true;
        refresh();
    });
}

void Daemon::systemctl(const QString &action, std::function<void(const QString &)> done)
{
    auto process = new QProcess(this);
    process->setProgram(qEnvironmentVariable("ENDEX_SYSTEMCTL", u"systemctl"_s));
    process->setArguments({u"--user"_s, action, u"endexd"_s});
    connect(process, &QProcess::finished, this, [process, action, done](int code, QProcess::ExitStatus status) {
        process->deleteLater();
        if (status == QProcess::NormalExit && code == 0) {
            done({});
            return;
        }
        const QString output = QString::fromLocal8Bit(process->readAllStandardError()).trimmed();
        done(tr("systemctl --user %1 endexd 失败（退出码 %2）：%3").arg(action).arg(code).arg(output));
    });
    connect(process, &QProcess::errorOccurred, this, [process, done](QProcess::ProcessError error) {
        if (error != QProcess::FailedToStart)
            return;
        process->deleteLater();
        done(tr("无法运行 systemctl：%1").arg(process->errorString()));
    });
    process->start();
}

void Daemon::setRestart(bool restarting, const QString &error)
{
    if (!restarting)
        m_restartTimeout.stop();
    m_restarted = false;
    m_restarting = restarting;
    m_restartError = error;
    Q_EMIT restartChanged();
}

void Daemon::setState(State state, const QString &error)
{
    if (state != Starting)
        m_startTimeout.stop();
    if (state == m_state && error == m_error)
        return;
    if (state != Ready)
        m_stale = true;
    // Not while a search is on its way, e.g. the one a summon sends: its answer is fresh anyway.
    const bool recovered = m_state != Ready && state == Ready && m_stale && !m_searching;
    if (recovered)
        m_stale = false;
    m_state = state;
    m_error = error;
    Q_EMIT stateChanged();
    if (recovered)
        Q_EMIT this->recovered();
}

void Daemon::setSearchError(const QString &error)
{
    if (error == m_searchError)
        return;
    m_searchError = error;
    Q_EMIT searchErrorChanged();
}

void Daemon::setSearching(bool searching)
{
    if (searching == m_searching)
        return;
    m_searching = searching;
    Q_EMIT searchingChanged();
}

bool Daemon::handleUnreachable(const QDBusError &error)
{
    switch (error.type()) {
    case QDBusError::ServiceUnknown:
    case QDBusError::NoReply:
    case QDBusError::Timeout:
    case QDBusError::Disconnected:
    case QDBusError::NoServer:
        break;
    default:
        if (!error.name().startsWith(u"org.freedesktop.DBus.Error.Spawn"_s) && error.name() != u"org.freedesktop.DBus.Error.NameHasNoOwner"_s)
            return false;
    }
    if (m_state != Starting)
        setState(Down, error.message());
    return true;
}
