import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

RowLayout {
    id: line

    required property Daemon daemon
    required property Results results
    property string modeLabel
    property bool showCounts

    function toast(message) {
        toastLabel.text = message
        toastTimer.restart()
    }

    function number(n) {
        return Number(n ?? 0).toLocaleString(Qt.locale(), "f", 0)
    }

    readonly property alias stateText: stateLabel.text
    readonly property var indexState: daemon.indexState
    readonly property bool writer: daemon.status.mode === "writer"
    readonly property bool unconfigured: writer && (daemon.status.sources ?? []).length === 0
    // ok, busy or error: the dot's colour.
    readonly property string health: {
        switch (daemon.state) {
        case Daemon.Ready:
        case Daemon.Empty:
            break
        case Daemon.Down:
        case Daemon.Broken:
            return "error"
        default:
            return "busy"
        }
        if (indexState.state === "error")
            return "error"
        if (unconfigured || ["indexing", "scanning", "paused", "needs_rebuild"].includes(indexState.state) || daemon.searchError)
            return "busy"
        return "ok"
    }

    spacing: Kirigami.Units.largeSpacing

    Timer {
        id: toastTimer
        interval: 2400
    }

    Rectangle {
        Layout.leftMargin: Kirigami.Units.largeSpacing * 1.5
        Layout.preferredWidth: Kirigami.Units.smallSpacing * 2
        Layout.preferredHeight: Kirigami.Units.smallSpacing * 2
        radius: width / 2
        color: line.health === "ok" ? Kirigami.Theme.positiveTextColor
            : line.health === "error" ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.neutralTextColor
    }

    QQC2.Label {
        id: stateLabel

        textFormat: Text.PlainText
        Layout.topMargin: Kirigami.Units.smallSpacing * 1.5
        Layout.bottomMargin: Kirigami.Units.smallSpacing * 1.5
        font: Kirigami.Theme.smallFont
        text: {
            const s = line.daemon.status
            const i = line.indexState
            switch (line.daemon.state) {
            case Daemon.Ready:
            case Daemon.Empty:
                if (line.daemon.searchError)
                    return qsTr("搜索出错：%1").arg(line.daemon.searchError)
                if (line.unconfigured)
                    return qsTr("尚未配置索引目录")
                switch (i.state) {
                case "error":
                    return qsTr("索引出错，已停止")
                case "needs_rebuild":
                    return qsTr("需要重建索引")
                case "paused":
                    return i.reason === "battery" ? qsTr("索引已暂停 · 电池供电") : qsTr("索引已暂停")
                case "scanning":
                    return qsTr("正在扫描目录…")
                case "indexing": {
                    const device = i.device === "gpu" ? (i.bulk ? qsTr("GPU 批量") : "GPU") : i.device === "cpu" ? "CPU" : ""
                    return (i.rebuilding ? qsTr("重建中 %1 / %2") : qsTr("索引中 %1 / %2")).arg(line.number(i.done)).arg(line.number(i.total))
                        + (device ? " · " + device : "")
                }
                }
                if (line.daemon.state === Daemon.Empty)
                    return qsTr("还没有索引")
                return qsTr("索引就绪 · %1 个文件 · %2 个片段")
                    .arg(line.number(s.doc_files + s.image_files)).arg(line.number(s.doc_chunks + s.image_chunks))
            case Daemon.Broken:
                return qsTr("索引不可用")
            case Daemon.Down:
                return qsTr("守护进程未运行")
            case Daemon.Starting:
                return qsTr("正在启动守护进程…")
            case Daemon.Rebuilding:
                return qsTr("正在重建索引…")
            default:
                return qsTr("正在连接守护进程…")
            }
        }
        elide: Text.ElideRight
        Layout.maximumWidth: Kirigami.Units.gridUnit * 24
    }

    // The config file is broken, or a change waits for a restart.
    QQC2.Label {
        id: notice

        textFormat: Text.PlainText
        visible: text !== ""
        Layout.maximumWidth: Kirigami.Units.gridUnit * 24
        font: Kirigami.Theme.smallFont
        color: Kirigami.Theme.negativeTextColor
        elide: Text.ElideMiddle
        text: {
            const s = line.daemon.status
            if (s.config_error)
                return qsTr("配置文件有错误：%1").arg(s.config_path)
            if (s.restart_required)
                return qsTr("改动要重启守护进程才生效（见设置）")
            return ""
        }
    }

    QQC2.Label {
        textFormat: Text.PlainText
        visible: line.showCounts && line.daemon.state === Daemon.Ready
        font: Kirigami.Theme.smallFont
        text: line.daemon.searching ? qsTr("搜索中…")
            : qsTr("%1 文档 · %2 图片 · %3 s").arg(line.results.docCount).arg(line.results.imageCount)
                .arg((line.daemon.latency / 1000).toFixed(2))
    }

    QQC2.Label {
        textFormat: Text.PlainText
        visible: line.daemon.state === Daemon.Ready && !line.unconfigured
        font: Kirigami.Theme.smallFont
        text: line.modeLabel
    }

    QQC2.Label {
        textFormat: Text.PlainText
        visible: line.daemon.state === Daemon.Ready && !line.unconfigured
        font: Kirigami.Theme.smallFont
        text: qsTr("查询设备 %1").arg(String(line.daemon.status.query_device ?? "cpu").toUpperCase())
        color: Kirigami.Theme.disabledTextColor
    }

    Item {
        Layout.fillWidth: true
    }

    QQC2.Label {
        id: toastLabel

        textFormat: Text.PlainText
        Layout.rightMargin: Kirigami.Units.largeSpacing * 1.5
        Layout.maximumWidth: line.width * 0.45
        visible: toastTimer.running
        font: Kirigami.Theme.smallFont
        color: Kirigami.Theme.positiveTextColor
        elide: Text.ElideMiddle
    }

    QQC2.Label {
        textFormat: Text.PlainText
        Layout.rightMargin: Kirigami.Units.largeSpacing * 1.5
        visible: !toastTimer.running && !notice.visible
        font: Kirigami.Theme.smallFont
        color: Kirigami.Theme.disabledTextColor
        text: qsTr("↑↓ 选择 · Tab 分组 · PgUp/PgDn 翻页 · ↵ 打开 · Esc 关闭")
    }
}
