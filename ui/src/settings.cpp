#include "settings.h"

#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDir>
#include <QFileInfo>

#include "daemon.h"

using namespace Qt::StringLiterals;

static QString expandHome(const QString &path)
{
    if (path == u"~"_s || path.startsWith(u"~/"_s))
        return QDir::homePath() + path.mid(1);
    return path;
}

QStringList Settings::pending() const
{
    QStringList keys;
    for (auto it = m_draft.cbegin(); it != m_draft.cend(); ++it) {
        if (m_saved.value(it.key()) != it.value())
            keys.append(it.key());
    }
    return keys;
}

void Settings::load()
{
    connect(callDaemon(this, u"GetSettings"_s, {}), &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *w) {
        w->deleteLater();
        const QDBusPendingReply<QVariantMap> reply = *w;
        if (reply.isError()) {
            Q_EMIT failed(reply.error().message());
            return;
        }
        const QStringList edited = pending();
        QVariantMap draft = fromDBus(reply.value()).toMap();
        m_saved = draft;
        for (const QString &key : edited)
            draft.insert(key, m_draft.value(key));
        m_draft = draft;
        m_loaded = true;
        Q_EMIT changed();
    });
}

void Settings::set(const QString &key, const QVariant &value)
{
    if (!m_saved.contains(key))
        return;
    const QVariant normal = normalized(key, value);
    if (m_draft.value(key) == normal)
        return;
    m_draft.insert(key, normal);
    Q_EMIT changed();
    QVariantMap errors = m_errors;
    errors.removeIf([&key](const auto &it) {
        return it.key() == key || it.key().startsWith(key + u'.');
    });
    setErrors(errors);
}

void Settings::revert()
{
    m_draft = m_saved;
    Q_EMIT changed();
    setErrors({});
}

void Settings::apply()
{
    // What is sent; an edit made while the call is out stays pending.
    QVariantMap sent;
    QVariantMap changes;
    for (const QString &key : pending()) {
        const QVariant value = m_draft.value(key);
        sent.insert(key, value);
        if (key == u"index.sources"_s) {
            QList<QVariantMap> sources;
            for (const QVariant &source : value.toList())
                sources.append(source.toMap());
            changes.insert(key, QVariant::fromValue(sources));
        } else {
            changes.insert(key, value);
        }
    }
    if (changes.isEmpty())
        return;
    setApplying(true);
    connect(callDaemon(this, u"ApplySettings"_s, {changes}), &QDBusPendingCallWatcher::finished, this, [this, sent](QDBusPendingCallWatcher *w) {
        w->deleteLater();
        setApplying(false);
        const QDBusPendingReply<QVariantMap> reply = *w;
        if (reply.isError()) {
            Q_EMIT failed(reply.error().message());
            return;
        }
        const QVariantMap result = fromDBus(reply.value()).toMap();
        setErrors(result.value(u"errors"_s).toMap());
        if (!result.value(u"applied"_s).toBool())
            return;
        // Until GetSettings answers, the saved values are what was written.
        m_saved.insert(sent);
        Q_EMIT changed();
        load();
        Q_EMIT applied(result.value(u"needs_rebuild"_s).toBool(), result.value(u"restart_required"_s).toBool());
    });
}

void Settings::checkPath(const QString &path)
{
    connect(callDaemon(this, u"CheckPath"_s, {path}), &QDBusPendingCallWatcher::finished, this, [this, path](QDBusPendingCallWatcher *w) {
        w->deleteLater();
        const QDBusPendingReply<QVariantMap> reply = *w;
        if (reply.isError())
            Q_EMIT pathChecked(path, {{u"error"_s, reply.error().message()}});
        else
            Q_EMIT pathChecked(path, fromDBus(reply.value()).toMap());
    });
}

void Settings::fetchFailedFiles()
{
    connect(callDaemon(this, u"FailedFiles"_s, {500u}), &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *w) {
        w->deleteLater();
        const QDBusPendingReply<QList<QVariantMap>> reply = *w;
        if (reply.isError())
            return;
        m_failedFiles.clear();
        for (const QVariantMap &file : reply.value())
            m_failedFiles.append(fromDBus(file));
        Q_EMIT failedFilesChanged();
    });
}

void Settings::fetchUsageLog()
{
    connect(callDaemon(this, u"UsageLog"_s, {50u}), &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *w) {
        w->deleteLater();
        const QDBusPendingReply<QStringList> reply = *w;
        m_usageLog = reply.isError() ? QStringList() : reply.value();
        Q_EMIT usageLogChanged();
    });
}

void Settings::clearUsageLog()
{
    connect(callDaemon(this, u"ClearUsageLog"_s, {}), &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *w) {
        w->deleteLater();
        const QDBusPendingReply<> reply = *w;
        if (reply.isError())
            Q_EMIT failed(reply.error().message());
        fetchUsageLog();
    });
}

QString Settings::configPath(const QUrl &url) const
{
    const QString path = QDir::cleanPath(url.toLocalFile());
    const QString home = QDir::homePath();
    if (path == home)
        return u"~"_s;
    if (path.startsWith(home + u'/'))
        return u"~"_s + path.mid(home.size());
    return path;
}

bool Settings::isModelFile(const QString &path) const
{
    const QFileInfo file(expandHome(path.trimmed()));
    return file.isFile() && file.suffix() == u"litertlm"_s;
}

// The draft keeps the saved value's types, so that an edit back to the saved value is no
// change.
QVariant Settings::normalized(const QString &key, const QVariant &value) const
{
    const QVariant saved = m_saved.value(key);
    switch (saved.typeId()) {
    case QMetaType::Bool:
        return value.toBool();
    case QMetaType::UInt:
        return value.toUInt();
    case QMetaType::Int:
        return value.toInt();
    case QMetaType::Double:
        return value.toDouble();
    case QMetaType::QString:
        return value.toString();
    case QMetaType::QStringList: {
        QStringList items;
        for (const QString &item : value.toStringList()) {
            if (!item.trimmed().isEmpty())
                items.append(item.trimmed());
        }
        return items;
    }
    case QMetaType::QVariantList: {
        // index.sources
        QVariantList sources;
        for (const QVariant &item : value.toList()) {
            const QVariantMap source = item.toMap();
            sources.append(QVariantMap{
                {u"path"_s, source.value(u"path"_s).toString()},
                {u"kind"_s, source.value(u"kind"_s, u"all"_s).toString()},
            });
        }
        return sources;
    }
    default:
        return value;
    }
}

void Settings::setErrors(const QVariantMap &errors)
{
    if (errors == m_errors)
        return;
    m_errors = errors;
    Q_EMIT errorsChanged();
}

void Settings::setApplying(bool applying)
{
    if (applying == m_applying)
        return;
    m_applying = applying;
    Q_EMIT applyingChanged();
}
