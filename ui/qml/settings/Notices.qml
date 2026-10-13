import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex
import "Markup.js" as Markup

// What applies to every settings page: the daemon unreachable or read-only, a broken config
// file, a change that waits for a restart, a needed rebuild, failed calls.
ColumnLayout {
    id: notices

    required property Daemon daemon
    required property Settings settings
    required property string page
    // A call that failed, until dismissed.
    required property string failure

    signal pageRequested(string id)
    signal copy(string text)
    signal openFile(string path)
    signal dismissed()

    readonly property var status: daemon.status
    readonly property bool reachable: daemon.state !== Daemon.Down && daemon.state !== Daemon.Starting

    spacing: Kirigami.Units.smallSpacing

    Notice {
        Layout.fillWidth: true
        // Not for the moment a restart takes.
        visible: !notices.reachable && !notices.daemon.restarting
        type: Kirigami.MessageType.Error
        text: qsTr("<b>连不上守护进程</b>，设置暂时不能读取或应用。") + (notices.daemon.error ? "<br>" + Markup.escape(notices.daemon.error) : "")
        actions: Kirigami.Action {
            text: notices.daemon.state === Daemon.Starting ? qsTr("正在启动…") : qsTr("启动")
            icon.name: "media-playback-start"
            enabled: notices.daemon.state !== Daemon.Starting
            onTriggered: notices.daemon.start()
        }
    }
    Notice {
        Layout.fillWidth: true
        visible: notices.reachable && notices.status.mode === "read_only"
        type: Kirigami.MessageType.Information
        text: qsTr("守护进程以只读方式运行（--data-dir），这里的改动不能应用。")
    }
    Notice {
        Layout.fillWidth: true
        visible: (notices.status.config_error ?? "") !== ""
        type: Kirigami.MessageType.Error
        text: qsTr("<b>配置文件有错误</b>：%1<br>守护进程仍在用上一次能用的设置。修好这个文件后会自动重新载入。<br><code>%2</code>")
            .arg(Markup.escape(notices.status.config_error)).arg(Markup.escape(notices.status.config_path))
        actions: Kirigami.Action {
            text: qsTr("打开配置文件")
            icon.name: "document-open"
            onTriggered: notices.openFile(notices.status.config_path)
        }
    }
    Notice {
        id: restartNotice

        readonly property bool writer: notices.status.mode === "writer"
        readonly property string failed: notices.daemon.restartError

        Layout.fillWidth: true
        visible: notices.status.restart_required === true
        type: failed ? Kirigami.MessageType.Error : Kirigami.MessageType.Warning
        text: (failed ? qsTr("<b>没能重启守护进程</b>：%1<br>").arg(Markup.escape(failed)) : "")
            + qsTr("有改动要重启守护进程才会生效（数据目录、嵌入库，或关掉了低优先级）。")
            + (writer ? qsTr("点「立即重启」，或在终端里运行：") : qsTr("在终端里运行："))
            + "<br><code>systemctl --user restart endexd</code>"
        actions: [
            Kirigami.Action {
                objectName: "restartNow"
                visible: restartNotice.writer
                enabled: !notices.daemon.restarting
                text: notices.daemon.restarting ? qsTr("正在重启…") : qsTr("立即重启")
                icon.name: "view-refresh"
                onTriggered: notices.daemon.restart()
            },
            Kirigami.Action {
                text: qsTr("复制命令")
                icon.name: "edit-copy"
                onTriggered: notices.copy("systemctl --user restart endexd")
            },
        ]
    }
    Notice {
        Layout.fillWidth: true
        visible: notices.page !== "status" && notices.daemon.indexState.state === "needs_rebuild"
        type: Kirigami.MessageType.Warning
        text: qsTr("模型相关的设置改了，已有索引要重建才会生效；在那之前新文件不会进索引。")
        actions: Kirigami.Action {
            text: qsTr("去「索引状态」重建")
            icon.name: "go-next"
            onTriggered: notices.pageRequested("status")
        }
    }
    Notice {
        Layout.fillWidth: true
        visible: (notices.settings.errors[""] ?? "") !== ""
        type: Kirigami.MessageType.Error
        text: qsTr("<b>config.toml 没有写入</b>：%1").arg(Markup.escape(notices.settings.errors[""]))
    }
    Notice {
        Layout.fillWidth: true
        // While endexd is unreachable the first notice says it all.
        visible: notices.failure !== "" && notices.reachable
        type: Kirigami.MessageType.Error
        text: qsTr("<b>这次操作没有完成</b>：%1").arg(Markup.escape(notices.failure))
        actions: Kirigami.Action {
            text: qsTr("知道了")
            onTriggered: notices.dismissed()
        }
    }
}
