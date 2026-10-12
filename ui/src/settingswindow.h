// The settings window: one per process, created when first shown.

#pragma once

#include <QObject>
#include <QPointer>
#include <QVariantMap>

class QQmlEngine;
class QQuickWindow;

class SettingsWindow : public QObject
{
    Q_OBJECT

public:
    // `properties`: the window's required properties.
    SettingsWindow(QQmlEngine *engine, const QVariantMap &properties, QObject *parent = nullptr);

    // Shows, raises and activates it on `page` (empty: the page it was on). An activation
    // token from a focused window (the popup) lets the compositor hand it the focus.
    void show(const QString &page, bool welcome, const QString &activationToken = {});
    QQuickWindow *window() const;

private:
    QQmlEngine *m_engine;
    QVariantMap m_properties;
    QPointer<QQuickWindow> m_window;
};
