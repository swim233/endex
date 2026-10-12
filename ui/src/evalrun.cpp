#include "evalrun.h"

#include <QCoreApplication>
#include <QFileInfo>
#include <QJsonDocument>
#include <QStandardPaths>

using namespace Qt::StringLiterals;

QString endexProgram()
{
    const QString sibling = QCoreApplication::applicationDirPath() + u"/endex"_s;
    return QFileInfo(sibling).isExecutable() ? sibling : QStandardPaths::findExecutable(u"endex"_s);
}

EvalRun::~EvalRun()
{
    stop();
}

QProcess *EvalRun::start(const QString &config, const QStringList &args)
{
    auto process = new QProcess(this);
    QStringList arguments;
    if (!config.isEmpty())
        arguments << u"--config"_s << config;
    process->setProgram(endexProgram());
    process->setArguments(arguments + args);
    connect(process, &QProcess::errorOccurred, this, [this, process](QProcess::ProcessError error) {
        if (error != QProcess::FailedToStart)
            return;
        setError(tr("无法运行 endex：%1").arg(process->errorString()));
        process->deleteLater();
        if (process == m_run) {
            m_run = nullptr;
            Q_EMIT runningChanged();
        }
    });
    process->start();
    return process;
}

void EvalRun::load(const QString &config)
{
    if (m_run)
        return;
    auto process = start(config, {u"eval"_s, u"--sets"_s, u"--saved"_s, u"--json"_s});
    connect(process, &QProcess::finished, this, [this, process] {
        process->deleteLater();
        finish(process);
    });
}

void EvalRun::run(const QString &config)
{
    if (m_run)
        return;
    setError({});
    m_stderr.clear();
    m_lastLine.clear();
    m_run = start(config, {u"eval"_s, u"--sets"_s, u"--json"_s});
    Q_EMIT runningChanged();
    QProcess *process = m_run;
    // Progress lines (`[2/5] scifact queries`); the last other line is the error message.
    connect(process, &QProcess::readyReadStandardError, this, [this, process] {
        m_stderr += process->readAllStandardError();
        qsizetype end;
        while ((end = m_stderr.indexOf('\n')) >= 0) {
            const QString line = QString::fromUtf8(m_stderr.left(end)).trimmed();
            m_stderr.remove(0, end + 1);
            if (line.startsWith(u'[')) {
                m_progress = line;
                Q_EMIT progressChanged();
            } else if (!line.isEmpty() && !line.startsWith(u"INFO:"_s)) {
                m_lastLine = line;
            }
        }
    });
    connect(process, &QProcess::finished, this, [this, process](int code, QProcess::ExitStatus status) {
        process->deleteLater();
        m_run = nullptr;
        if (status != QProcess::NormalExit)
            setError(tr("评测已停止"));
        else if (code != 0)
            setError(m_lastLine.startsWith(u"Error: "_s) ? m_lastLine.mid(7) : m_lastLine);
        else
            finish(process);
        m_progress.clear();
        Q_EMIT progressChanged();
        Q_EMIT runningChanged();
    });
}

void EvalRun::stop()
{
    if (m_run && m_run->state() != QProcess::NotRunning)
        m_run->kill();
}

bool EvalRun::finish(QProcess *process)
{
    const QByteArray output = process->readAllStandardOutput().trimmed();
    // `null`: nothing saved yet.
    if (output == "null" || output.isEmpty()) {
        m_results.clear();
        Q_EMIT resultsChanged();
        return true;
    }
    QJsonParseError parse;
    const auto json = QJsonDocument::fromJson(output, &parse);
    if (parse.error != QJsonParseError::NoError) {
        setError(tr("读不懂 endex eval 的输出：%1").arg(parse.errorString()));
        return false;
    }
    m_results = json.toVariant().toMap();
    Q_EMIT resultsChanged();
    return true;
}

void EvalRun::setError(const QString &error)
{
    if (error == m_error)
        return;
    m_error = error;
    Q_EMIT errorChanged();
}
