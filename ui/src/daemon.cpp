#include "daemon.h"

#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QPoint>
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
}

static QDBusPendingCallWatcher *call(QObject *owner, const QString &method, const QVariantList &args)
{
    auto message = QDBusMessage::createMethodCall(Service, Path, Interface, method);
    message.setArguments(args);
    auto pending = QDBusConnection::sessionBus().asyncCall(message, CallTimeoutMs);
    return new QDBusPendingCallWatcher(pending, owner);
}

void Daemon::refresh()
{
    connect(call(this, u"Status"_s, {}), &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *w) {
        w->deleteLater();
        const QDBusPendingReply<QVariantMap> reply = *w;
        if (reply.isError()) {
            if (!handleUnreachable(reply.error()))
                setState(Broken, reply.error().message());
            return;
        }
        m_status = reply.value();
        Q_EMIT statusChanged();
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
    auto watcher = call(this, u"Search"_s, {query, mode, opts});
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
    connect(call(this, u"Preview"_s, {id, query}), &QDBusPendingCallWatcher::finished, this, [=, this](QDBusPendingCallWatcher *w) {
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

void Daemon::start()
{
    setState(Starting);
    m_startTimeout.start();
    auto systemctl = new QProcess(this);
    systemctl->setProgram(qEnvironmentVariable("ENDEX_SYSTEMCTL", u"systemctl"_s));
    systemctl->setArguments({u"--user"_s, u"start"_s, u"endexd"_s});
    connect(systemctl, &QProcess::finished, this, [this, systemctl](int code, QProcess::ExitStatus status) {
        systemctl->deleteLater();
        if (status == QProcess::NormalExit && code == 0) {
            // Type=dbus: the name is already on the bus.
            refresh();
            return;
        }
        const QString output = QString::fromLocal8Bit(systemctl->readAllStandardError()).trimmed();
        setState(Down, tr("systemctl --user start endexd 失败（退出码 %1）：%2").arg(code).arg(output));
    });
    connect(systemctl, &QProcess::errorOccurred, this, [this, systemctl](QProcess::ProcessError error) {
        if (error != QProcess::FailedToStart)
            return;
        systemctl->deleteLater();
        setState(Down, tr("无法运行 systemctl：%1").arg(systemctl->errorString()));
    });
    systemctl->start();
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
