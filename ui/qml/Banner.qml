pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

// The indexer above the results: a first build or rebuild in progress, a pause on battery, an
// error, a rebuild that is needed or waits.
Rectangle {
    id: banner

    required property Daemon daemon
    // resources.profile, for the hint about OCR speed.
    property string profile

    signal settingsRequested(string page)

    readonly property var s: daemon.indexState
    readonly property var status: daemon.status
    readonly property string kind: {
        if (daemon.state === Daemon.Down || daemon.state === Daemon.Starting || status.mode !== "writer")
            return ""
        switch (s.state) {
        case "error":
            return "error"
        case "needs_rebuild":
            return "needs_rebuild"
        case "paused":
            return s.rebuild_pending ? "rebuild_pending" : s.reason === "battery" ? "battery" : "paused"
        case "indexing":
        case "scanning":
            // Not for a few changed files: the status line shows those.
            return s.bulk || s.rebuilding || s.total >= 20 ? "indexing" : ""
        default:
            return ""
        }
    }
    readonly property real fraction: s.total > 0 ? s.done / s.total : 0
    readonly property color tone: kind === "error" ? Kirigami.Theme.negativeTextColor
        : kind === "indexing" ? Kirigami.Theme.highlightColor : Kirigami.Theme.neutralTextColor

    function number(n) {
        return Number(n ?? 0).toLocaleString(Qt.locale(), "f", 0)
    }

    visible: kind !== ""
    implicitHeight: row.implicitHeight + Look.smallSpacing * 2
    radius: Look.radius
    color: Qt.alpha(tone, 0.12)
    border.color: Qt.alpha(tone, 0.6)

    RowLayout {
        id: row

        anchors.fill: parent
        anchors.leftMargin: Look.largeSpacing
        anchors.rightMargin: Look.smallSpacing
        spacing: Look.largeSpacing

        Kirigami.Icon {
            implicitWidth: Kirigami.Units.iconSizes.small
            implicitHeight: implicitWidth
            color: banner.tone
            source: {
                switch (banner.kind) {
                case "error":
                    return "dialog-error"
                case "battery":
                    return "battery-caution"
                case "paused":
                    return "media-playback-pause"
                default:
                    return "view-refresh"
                }
            }
        }
        QQC2.Label {
            textFormat: Text.PlainText
            font.bold: true
            text: {
                switch (banner.kind) {
                case "error":
                    return qsTr("索引出错，已停止")
                case "needs_rebuild":
                    return qsTr("需要重建索引")
                case "rebuild_pending":
                    return qsTr("重建索引在等待")
                case "battery":
                    return qsTr("电池供电，已暂停索引")
                case "paused":
                    return qsTr("索引已暂停")
                default:
                    return banner.s.state === "scanning" ? qsTr("正在扫描目录")
                        : banner.s.rebuilding ? qsTr("正在重建索引") : qsTr("正在建立索引")
                }
            }
        }
        QQC2.Label {
            Layout.fillWidth: banner.kind !== "indexing"
            Layout.maximumWidth: banner.kind === "error" ? banner.width * 0.45 : -1
            textFormat: Text.PlainText
            elide: Text.ElideRight
            text: {
                const s = banner.s
                switch (banner.kind) {
                case "error":
                    return s.reason
                case "needs_rebuild":
                    return { model: qsTr("已修改嵌入模型，等待重建索引"), prefix_style: qsTr("已修改前缀风格，等待重建索引"), ocr: qsTr("已修改 OCR 设置，等待重建索引") }[s.reason] ?? s.reason
                case "rebuild_pending":
                    return s.reason === "battery" ? qsTr("电池供电时不重建；接上电源后开始") : qsTr("继续索引后开始")
                case "battery": {
                    const percent = banner.status.battery_percent >= 0 ? qsTr("剩余 %1%；").arg(Math.round(banner.status.battery_percent)) : ""
                    return percent + qsTr("接上电源后自动继续")
                }
                case "paused":
                    return s.total > 0 ? qsTr("%1 / %2 个文件").arg(banner.number(s.done)).arg(banner.number(s.total)) : ""
                default:
                    return s.state === "scanning" ? "" : qsTr("%1 / %2 个文件（%3%）").arg(banner.number(s.done)).arg(banner.number(s.total))
                        .arg(Math.floor(banner.fraction * 100))
                }
            }
        }
        QQC2.ProgressBar {
            visible: banner.kind === "indexing"
            Layout.preferredWidth: Kirigami.Units.gridUnit * 8
            from: 0
            to: 1
            value: banner.fraction
            indeterminate: banner.s.state === "scanning"
        }
        QQC2.Label {
            visible: banner.kind === "indexing" && banner.s.state === "indexing"
            Layout.fillWidth: true
            textFormat: Text.PlainText
            elide: Text.ElideRight
            font: Look.smallFont
            text: {
                const s = banner.s
                const device = s.device === "gpu" ? "GPU" : s.device === "cpu" ? "CPU" : ""
                const parts = [s.bulk ? qsTr("批量嵌入") : "", device]
                if (s.eta >= 0)
                    parts.push(s.eta < 60 ? qsTr("不到 1 分钟") : qsTr("约剩 %1 分钟").arg(Math.round(s.eta / 60)))
                return parts.filter(p => p).join(" · ")
            }
        }
        Item {
            visible: banner.kind === "indexing" && banner.s.state !== "indexing"
            Layout.fillWidth: true
        }
        QQC2.Label {
            visible: banner.kind === "indexing"
            textFormat: Text.PlainText
            font: Look.smallFont
            color: Kirigami.Theme.neutralTextColor
            text: qsTr("搜索结果可能不完整")
        }
        QQC2.Label {
            // A first build with many images: OCR dominates.
            visible: banner.kind === "indexing" && banner.profile !== "" && banner.profile !== "heavy" && (banner.s.images ?? 0) >= 100
            Layout.rightMargin: Look.smallSpacing
            textFormat: Text.PlainText
            font: Look.smallFont
            color: Kirigami.Theme.neutralTextColor
            text: qsTr("图片较多，索引耗时较长")
        }
        QQC2.Button {
            objectName: "bannerContinue"
            visible: banner.kind === "battery" || (banner.kind === "rebuild_pending" && banner.s.reason === "battery")
            focusPolicy: Qt.NoFocus
            text: qsTr("本次继续")
            onClicked: banner.daemon.control("ContinueOnBattery")
        }
        QQC2.Button {
            objectName: "bannerResume"
            visible: banner.kind === "error" || banner.kind === "paused" || (banner.kind === "rebuild_pending" && banner.s.reason !== "battery")
            focusPolicy: Qt.NoFocus
            text: qsTr("继续")
            onClicked: banner.daemon.control("Resume")
        }
        QQC2.Button {
            visible: banner.kind === "battery" || banner.kind === "rebuild_pending"
            flat: true
            focusPolicy: Qt.NoFocus
            text: qsTr("性能")
            onClicked: banner.settingsRequested("resource")
        }
        QQC2.Button {
            visible: banner.kind === "needs_rebuild"
            focusPolicy: Qt.NoFocus
            text: qsTr("去重建")
            onClicked: banner.settingsRequested("status")
        }
    }
}
