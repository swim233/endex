// endex-ui: the search popup and the settings window. One instance per session;
// org.endex.Ui1.Toggle shows or hides the popup, ShowSettings opens the settings.

#include <QApplication>
#include <QCommandLineParser>
#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusMetaType>
#include <QNetworkAccessManager>
#include <QNetworkRequest>
#include <QQmlApplicationEngine>
#include <QQmlNetworkAccessManagerFactory>
#include <QQuickStyle>
#include <QQuickWindow>

#include "appearance.h"
#include "daemon.h"
#include "evalrun.h"
#include "popup.h"
#include "results.h"
#include "settings.h"
#include "settingswindow.h"
#include "uidebug.h"

using namespace Qt::StringLiterals;

static const auto Service = u"org.endex.Ui1"_s;
static const auto Path = u"/org/endex/Ui"_s;

// Defence in depth: indexed text is shown as plain text, but should rich text ever carry an
// <img src="http://…">, QML must not fetch it. Only local files and resources load.
class LocalOnlyNetwork : public QNetworkAccessManager
{
public:
    using QNetworkAccessManager::QNetworkAccessManager;

protected:
    QNetworkReply *createRequest(Operation op, const QNetworkRequest &request, QIODevice *data) override
    {
        const QString scheme = request.url().scheme();
        if (scheme == u"file"_s || scheme == u"qrc"_s)
            return QNetworkAccessManager::createRequest(op, request, data);
        QNetworkRequest refused(request);
        refused.setUrl(QUrl(u"refused:"_s));
        return QNetworkAccessManager::createRequest(op, refused, data);
    }
};

class LocalOnlyNetworkFactory : public QQmlNetworkAccessManagerFactory
{
public:
    QNetworkAccessManager *create(QObject *parent) override
    {
        return new LocalOnlyNetwork(parent);
    }
};

int main(int argc, char *argv[])
{
    // QApplication, not QGuiApplication: the desktop Qt Quick Controls style draws with QStyle.
    QApplication app(argc, argv);
    app.setApplicationName(u"endex-ui"_s);
    app.setApplicationDisplayName(u"Endex"_s);
    app.setDesktopFileName(u"endex-ui"_s);
    // The popup is hidden most of the time, and KIO jobs (open, reveal) hold event-loop locks:
    // releasing the last one must not quit the app.
    app.setQuitOnLastWindowClosed(false);
    app.setQuitLockEnabled(false);
    if (qEnvironmentVariableIsEmpty("QT_QUICK_CONTROLS_STYLE"))
        QQuickStyle::setStyle(u"org.kde.desktop"_s);

    QCommandLineParser args;
    args.addHelpOption();
    args.addOption({u"show"_s, u"Show the popup right away."_s});
    args.process(app);

    auto bus = QDBusConnection::sessionBus();
    const auto toggleRunning = [&bus] {
        bus.call(QDBusMessage::createMethodCall(Service, Path, Service, u"Toggle"_s));
    };
    if (bus.interface()->isServiceRegistered(Service)) {
        toggleRunning();
        return 0;
    }
    qDBusRegisterMetaType<QList<QVariantMap>>();

    Results results;
    Daemon daemon(&results);
    Settings settings;
    // The applied look: usually in before the first Toggle is delivered.
    settings.load();
    Appearance appearance(&settings);
    EvalRun evalRun;
    Popup popup;
    QQmlApplicationEngine engine;
    LocalOnlyNetworkFactory network;
    engine.setNetworkAccessManagerFactory(&network);
    engine.setInitialProperties({
        {u"popup"_s, QVariant::fromValue(&popup)},
        {u"daemon"_s, QVariant::fromValue(&daemon)},
        {u"results"_s, QVariant::fromValue(&results)},
        {u"settings"_s, QVariant::fromValue(&settings)},
    });
    engine.loadFromModule("Endex", "Main");
    if (engine.rootObjects().isEmpty())
        return 1;
    QObject *root = engine.rootObjects().constFirst();
    SettingsWindow settingsWindow(&engine,
                                  {
                                      {u"popup"_s, QVariant::fromValue(&popup)},
                                      {u"daemon"_s, QVariant::fromValue(&daemon)},
                                      {u"settings"_s, QVariant::fromValue(&settings)},
                                      {u"evalRun"_s, QVariant::fromValue(&evalRun)},
                                  });
    popup.attach(qobject_cast<QQuickWindow *>(root), &settingsWindow, &appearance);

    // The object goes first: D-Bus activation delivers the waiting Toggle as soon as the name
    // is ours.
    bus.registerObject(Path, &popup, QDBusConnection::ExportScriptableSlots);
    UiDebug debug(root, &settingsWindow);
    if (qgetenv("ENDEX_UI_DEBUG") == "1")
        bus.registerObject(u"/org/endex/UiDebug"_s, &debug, QDBusConnection::ExportScriptableSlots);
    if (!bus.registerService(Service)) {
        toggleRunning();
        return 0;
    }
    daemon.refresh();
    if (args.isSet(u"show"_s))
        popup.Show({});
    return app.exec();
}
