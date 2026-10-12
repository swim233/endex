#include "uidebug.h"

#include <QQuickItem>
#include <QQuickWindow>
#include <QUrl>

#include "settingswindow.h"

using namespace Qt::StringLiterals;

static bool usable(QObject *object)
{
    return object->property("visible").toBool() && object->property("enabled").toBool();
}

// Delegates are only in the item tree; actions and dialogs only among the QObject children.
static QObject *findIn(QQuickItem *item, const QString &name)
{
    if (item->objectName() == name && usable(item))
        return item;
    for (QQuickItem *child : item->childItems()) {
        if (QObject *found = findIn(child, name))
            return found;
    }
    return nullptr;
}

static QString call(QObject *object, const char *method, const QVariant &arg = {})
{
    QVariant result;
    if (arg.isValid())
        QMetaObject::invokeMethod(object, method, Q_RETURN_ARG(QVariant, result), Q_ARG(QVariant, arg));
    else
        QMetaObject::invokeMethod(object, method, Q_RETURN_ARG(QVariant, result));
    return result.toString();
}

UiDebug::UiDebug(QObject *popup, SettingsWindow *settings)
    : m_popup(popup)
    , m_settings(settings)
{
}

QString UiDebug::State()
{
    return call(m_popup, "debugState");
}

QString UiDebug::SettingsState()
{
    return m_settings->window() ? call(m_settings->window(), "debugState") : u"none"_s;
}

QString UiDebug::AddFolder(const QString &path)
{
    return m_settings->window() ? call(m_settings->window(), "addFolder", QUrl::fromLocalFile(path)) : u"none"_s;
}

bool UiDebug::Click(const QString &objectName)
{
    QObject *target = find(objectName);
    if (!target)
        return false;
    const bool button = target->metaObject()->indexOfMethod("click()") >= 0;
    return QMetaObject::invokeMethod(target, button ? "click" : "trigger");
}

bool UiDebug::SetText(const QString &objectName, const QString &text)
{
    auto *item = qobject_cast<QQuickItem *>(find(objectName));
    if (!item)
        return false;
    item->forceActiveFocus();
    item->setProperty("text", text);
    // What typing would also signal (the popup counts it as the user's search).
    if (item->metaObject()->indexOfSignal("textEdited()") >= 0)
        QMetaObject::invokeMethod(item, "textEdited");
    if (item->metaObject()->indexOfSignal("accepted()") >= 0)
        QMetaObject::invokeMethod(item, "accepted");
    return true;
}

void UiDebug::CloseSettings()
{
    if (m_settings->window())
        m_settings->window()->close();
}

QObject *UiDebug::find(const QString &objectName) const
{
    for (QObject *window : {m_popup, static_cast<QObject *>(m_settings->window())}) {
        auto *quick = qobject_cast<QQuickWindow *>(window);
        if (!quick)
            continue;
        if (QObject *found = findIn(quick->contentItem(), objectName))
            return found;
        for (QObject *object : quick->findChildren<QObject *>(objectName)) {
            if (usable(object))
                return object;
        }
    }
    return nullptr;
}
