// 「运行评测」: `endex eval --sets` in the background, and the results it saved last.

#pragma once

#include <QObject>
#include <QPointer>
#include <QProcess>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

// The `endex` CLI installed next to endex-ui, else the one on PATH (empty if none).
QString endexProgram();

class EvalRun : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_UNCREATABLE("Created by main")
    // The last saved run: time, model, limit, sets [{set, queries, kind, count, modes}];
    // empty if there is none.
    Q_PROPERTY(QVariantMap results READ results NOTIFY resultsChanged)
    Q_PROPERTY(bool running READ running NOTIFY runningChanged)
    // The set being evaluated, e.g. `[2/5] scifact queries`.
    Q_PROPERTY(QString progress READ progress NOTIFY progressChanged)
    // Why the last run or load failed.
    Q_PROPERTY(QString error READ error NOTIFY errorChanged)

public:
    using QObject::QObject;
    ~EvalRun() override;

    QVariantMap results() const { return m_results; }
    bool running() const { return m_run; }
    QString progress() const { return m_progress; }
    QString error() const { return m_error; }

    // `config`: the daemon's config file, so that the CLI finds the same data dir.
    Q_INVOKABLE void load(const QString &config);
    Q_INVOKABLE void run(const QString &config);
    Q_INVOKABLE void stop();

Q_SIGNALS:
    void resultsChanged();
    void runningChanged();
    void progressChanged();
    void errorChanged();

private:
    QProcess *start(const QString &config, const QStringList &args);
    // Parses stdout once the process ended; false with `m_error` set otherwise.
    bool finish(QProcess *process);
    void setError(const QString &error);

    QVariantMap m_results;
    QPointer<QProcess> m_run;
    QByteArray m_stderr;
    QString m_lastLine;
    QString m_progress;
    QString m_error;
};
