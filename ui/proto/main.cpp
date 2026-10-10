// Phase 0 prototype: a layer-shell popup summoned over D-Bus (org.endex.Ui1.Toggle).
// It only has to prove focus on summon, fcitx5 input, clipboard and Esc on this KWin.

#include <QClipboard>
#include <QCommandLineParser>
#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusMessage>
#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQuickItem>
#include <QQuickWindow>

#include <LayerShellQt/Window>

using namespace Qt::StringLiterals;

static const auto Service = u"org.endex.Ui1"_s;
static const auto Path = u"/org/endex/Ui"_s;

class Popup : public QObject
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.endex.Ui1")

public:
    void attach(QQuickWindow *window)
    {
        m_window = window;
        // Top, not overlay: KWin stacks the input method's candidate window below overlay
        // surfaces, so fcitx5 candidates would be hidden behind the popup. A fullscreen window
        // stops being "active" once the popup takes the keyboard, so the popup still shows on
        // top of it. Prototype knob for comparison: ENDEX_PROTO_LAYER=top|overlay|none.
        const auto mode = qEnvironmentVariable("ENDEX_PROTO_LAYER", u"top"_s);
        if (mode == u"none"_s)
            return;
        auto layer = LayerShellQt::Window::get(window);
        layer->setScope(u"endex"_s);
        layer->setLayer(mode == u"overlay"_s ? LayerShellQt::Window::LayerOverlay : LayerShellQt::Window::LayerTop);
        layer->setAnchors(LayerShellQt::Window::AnchorTop);
        layer->setMargins({0, 120, 0, 0});
        // Exclusive: the compositor must give the surface keyboard focus while it is mapped.
        // On-demand focus would depend on KWin's focus-stealing prevention, and a D-Bus call
        // from a global shortcut carries no activation token.
        // Prototype knob for comparison: ENDEX_PROTO_KEYBOARD=exclusive|ondemand.
        layer->setKeyboardInteractivity(qEnvironmentVariable("ENDEX_PROTO_KEYBOARD") == u"ondemand"_s
                                            ? LayerShellQt::Window::KeyboardInteractivityOnDemand
                                            : LayerShellQt::Window::KeyboardInteractivityExclusive);
        layer->setWantsToBeOnActiveScreen(true);
    }

    Q_INVOKABLE void hide() { m_window->hide(); }

    Q_INVOKABLE void copy(const QString &text) { QGuiApplication::clipboard()->setText(text); }

public Q_SLOTS:
    Q_SCRIPTABLE void Toggle()
    {
        if (m_window->isVisible())
            hide();
        else
            Show({});
    }

    Q_SCRIPTABLE void Show(const QString &query)
    {
        m_window->show();
        m_window->requestActivate();
        QMetaObject::invokeMethod(m_window, "summoned", Q_ARG(QVariant, query));
    }

    // Prototype only: lets a test script check focus without looking at the screen.
    Q_SCRIPTABLE QString State() const
    {
        auto focus = m_window->activeFocusItem();
        return u"visible=%1 active=%2 focus=%3 text=%4 preedit=%5"_s.arg(m_window->isVisible())
            .arg(m_window->isActive())
            .arg(focus ? focus->objectName() : u"-"_s,
                 m_window->property("searchText").toString(),
                 m_window->property("preeditText").toString());
    }

private:
    QQuickWindow *m_window = nullptr;
};

int main(int argc, char *argv[])
{
    QGuiApplication app(argc, argv);
    app.setApplicationName(u"endex-ui-proto"_s);
    app.setDesktopFileName(u"endex-ui"_s);

    QCommandLineParser args;
    args.addOption({u"show"_s, u"Show the popup right away."_s});
    args.process(app);

    auto bus = QDBusConnection::sessionBus();
    if (!bus.registerService(Service)) {
        // Already running: behave like a second summon.
        bus.call(QDBusMessage::createMethodCall(Service, Path, Service, u"Toggle"_s));
        return 0;
    }

    Popup popup;
    QQmlApplicationEngine engine;
    engine.setInitialProperties({{u"popup"_s, QVariant::fromValue(&popup)}});
    engine.loadFromModule("EndexProto", "Main");
    if (engine.rootObjects().isEmpty())
        return 1;
    popup.attach(qobject_cast<QQuickWindow *>(engine.rootObjects().constFirst()));
    bus.registerObject(Path, &popup, QDBusConnection::ExportScriptableSlots);

    if (args.isSet(u"show"_s))
        popup.Show({});
    return app.exec();
}

#include "main.moc"
