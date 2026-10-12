// The settings window's client of endexd: GetSettings into a draft that is applied
// (ApplySettings) or reverted as a whole, plus what the pages list (failed files, usage log,
// CheckPath).

#pragma once

#include <QObject>
#include <QUrl>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

class Settings : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_UNCREATABLE("Created by main")
    // The settings as endexd has them (flat keys, see the README).
    Q_PROPERTY(QVariantMap saved READ saved NOTIFY changed)
    // `saved` plus the edits not applied yet.
    Q_PROPERTY(QVariantMap draft READ draft NOTIFY changed)
    // Keys whose draft differs from the saved value.
    Q_PROPERTY(QStringList pending READ pending NOTIFY changed)
    Q_PROPERTY(bool loaded READ loaded NOTIFY changed)
    // Problems of the last ApplySettings by key (`index.sources.2`, `""` for the file); editing
    // a key clears its own.
    Q_PROPERTY(QVariantMap errors READ errors NOTIFY errorsChanged)
    Q_PROPERTY(bool applying READ applying NOTIFY applyingChanged)
    // FailedFiles(): path, error, time.
    Q_PROPERTY(QVariantList failedFiles READ failedFiles NOTIFY failedFilesChanged)
    // The last lines of the usage log (JSON), oldest first.
    Q_PROPERTY(QStringList usageLog READ usageLog NOTIFY usageLogChanged)

public:
    using QObject::QObject;

    QVariantMap saved() const { return m_saved; }
    QVariantMap draft() const { return m_draft; }
    QStringList pending() const;
    bool loaded() const { return m_loaded; }
    QVariantMap errors() const { return m_errors; }
    bool applying() const { return m_applying; }
    QVariantList failedFiles() const { return m_failedFiles; }
    QStringList usageLog() const { return m_usageLog; }

    // Unapplied edits survive a reload.
    Q_INVOKABLE void load();
    Q_INVOKABLE void set(const QString &key, const QVariant &value);
    Q_INVOKABLE void revert();
    Q_INVOKABLE void apply();
    Q_INVOKABLE void checkPath(const QString &path);
    Q_INVOKABLE void fetchFailedFiles();
    Q_INVOKABLE void fetchUsageLog();
    Q_INVOKABLE void clearUsageLog();
    // A folder from the folder dialog as config.toml writes it (`~/…` inside the home folder).
    Q_INVOKABLE QString configPath(const QUrl &url) const;
    // An existing `.litertlm` file (`~/` expanded).
    Q_INVOKABLE bool isModelFile(const QString &path) const;

Q_SIGNALS:
    void changed();
    void errorsChanged();
    void applyingChanged();
    void failedFilesChanged();
    void usageLogChanged();
    // config.toml was written.
    void applied(bool needsRebuild, bool restartRequired);
    // A call did not reach endexd or was refused (read-only daemon).
    void failed(const QString &message);
    void pathChecked(const QString &path, const QVariantMap &result);

private:
    QVariant normalized(const QString &key, const QVariant &value) const;
    void setErrors(const QVariantMap &errors);
    void setApplying(bool applying);

    QVariantMap m_saved;
    QVariantMap m_draft;
    bool m_loaded = false;
    QVariantMap m_errors;
    bool m_applying = false;
    QVariantList m_failedFiles;
    QStringList m_usageLog;
};
