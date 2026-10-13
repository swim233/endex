pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex
import "Markup.js" as Markup

// 索引状态: progress with pause and continue, counts, failed files, and rebuilding.
ColumnLayout {
    id: page

    required property Daemon daemon
    required property Settings settings

    signal pageRequested(string id)

    readonly property var status: daemon.status
    readonly property var s: daemon.indexState
    readonly property bool writer: status.mode === "writer"
    readonly property bool noSources: writer && (status.sources ?? []).length === 0
    readonly property var unavailable: (status.sources ?? []).filter(src => !src.exists).map(src => src.path)
    readonly property real fraction: s.total > 0 ? s.done / s.total : 0
    readonly property var imageExtensions: ["png", "jpg", "jpeg", "webp"]
    property bool confirmRebuild
    // Failed files sent to Retry, until the list is fetched again.
    property var queued: []

    function number(n) {
        return Number(n ?? 0).toLocaleString(Qt.locale(), "f", 0)
    }

    function bytes(n) {
        const units = ["B", "KB", "MB", "GB", "TB"]
        let i = 0
        let v = Number(n ?? 0)
        while (v >= 1024 && i < units.length - 1) {
            v /= 1024
            i++
        }
        return `${v.toFixed(i === 0 ? 0 : 1)} ${units[i]}`
    }

    function minutes(eta) {
        return eta < 0 ? "" : eta < 60 ? qsTr("不到 1 分钟") : qsTr("约剩 %1 分钟").arg(Math.round(eta / 60))
    }

    function when(unix) {
        return unix > 0 ? Qt.formatDateTime(new Date(unix * 1000), "yyyy-MM-dd HH:mm") : ""
    }

    function formatCount(filter) {
        const formats = status.formats ?? {}
        return Object.keys(formats).filter(filter).reduce((n, ext) => n + formats[ext], 0)
    }

    Component.onCompleted: settings.fetchFailedFiles()

    Timer {
        interval: 5000
        repeat: true
        running: page.visible
        onTriggered: page.settings.fetchFailedFiles()
    }

    Connections {
        target: page.settings

        function onFailedFilesChanged() {
            page.queued = []
        }
    }

    spacing: Kirigami.Units.largeSpacing

    Notice {
        Layout.fillWidth: true
        visible: page.daemon.state === Daemon.Down || page.daemon.state === Daemon.Starting
        type: Kirigami.MessageType.Error
        text: qsTr("无法连接至守护进程，请检查守护进程状态")
    }
    Notice {
        Layout.fillWidth: true
        visible: page.noSources
        text: qsTr("还没有索引目录。先去「索引目录」添加。")
        actions: Kirigami.Action {
            text: qsTr("去「索引目录」")
            icon.name: "go-next"
            onTriggered: page.pageRequested("dirs")
        }
    }
    Notice {
        Layout.fillWidth: true
        visible: page.s.state === "error"
        type: Kirigami.MessageType.Error
        text: qsTr("<b>索引出错，已停止</b>：%1<br>请排除问题后继续索引。").arg(Markup.escape(page.s.reason))
        actions: Kirigami.Action {
            objectName: "resumeAfterError"
            text: qsTr("继续")
            icon.name: "media-playback-start"
            onTriggered: page.daemon.control("Resume")
        }
    }
    Notice {
        Layout.fillWidth: true
        visible: page.s.state === "needs_rebuild"
        type: Kirigami.MessageType.Warning
        text: {
            const what = { model: qsTr("嵌入模型"), prefix_style: qsTr("前缀风格"), ocr: qsTr("OCR 设置") }[page.s.reason] ?? page.s.reason
            return qsTr("%1发生更改：需要重建索引").arg(what)
        }
        actions: Kirigami.Action {
            text: qsTr("重建索引…")
            icon.name: "view-refresh"
            onTriggered: page.confirmRebuild = true
        }
    }

    Card {
        visible: !page.noSources && page.s.state !== "error" && page.s.state !== "needs_rebuild"
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.largeSpacing

            QQC2.Label {
                textFormat: Text.PlainText
                font.bold: true
                text: {
                    switch (page.s.state) {
                    case "paused":
                        return page.s.rebuild_pending ? qsTr("重建在等待") : qsTr("已暂停")
                    case "scanning":
                        return qsTr("正在扫描目录")
                    case "indexing":
                        return page.s.rebuilding ? qsTr("正在重建") : qsTr("正在索引")
                    default:
                        return qsTr("索引就绪")
                    }
                }
            }
            QQC2.Label {
                textFormat: Text.PlainText
                font: Look.smallFont
                color: page.s.reason === "battery" ? Kirigami.Theme.neutralTextColor : Kirigami.Theme.disabledTextColor
                text: {
                    switch (page.s.state) {
                    case "paused":
                        return page.s.reason === "battery" ? qsTr("电池供电") : qsTr("手动暂停")
                    case "indexing":
                        return page.s.device === "gpu" ? (page.s.bulk ? qsTr("GPU 批量") : "GPU") : page.s.device === "cpu" ? "CPU" : ""
                    case "scanning":
                        return ""
                    default:
                        return page.daemon.state === Daemon.Ready ? qsTr("最新") : ""
                    }
                }
            }
            Hint {
                text: page.s.state === "indexing" ? page.minutes(page.s.eta ?? -1)
                    : page.s.state === "idle" && page.status.last_indexed > 0 ? qsTr("上次更新：%1").arg(page.when(page.status.last_indexed)) : ""
            }
            QQC2.Button {
                objectName: "pause"
                visible: page.writer && (page.s.state === "indexing" || page.s.state === "scanning")
                text: qsTr("暂停")
                icon.name: "media-playback-pause"
                onClicked: page.daemon.control("Pause")
            }
            QQC2.Button {
                objectName: "continueOnBattery"
                visible: page.s.state === "paused" && page.s.reason === "battery"
                text: qsTr("本次继续")
                icon.name: "media-playback-start"
                onClicked: page.daemon.control("ContinueOnBattery")
            }
            QQC2.Button {
                objectName: "resume"
                visible: page.s.state === "paused" && page.s.reason === "user"
                text: qsTr("继续")
                icon.name: "media-playback-start"
                onClicked: page.daemon.control("Resume")
            }
        }
        QQC2.ProgressBar {
            Layout.fillWidth: true
            from: 0
            to: 1
            value: page.s.state === "idle" ? 1 : page.fraction
            indeterminate: page.s.state === "scanning"
        }
        Hint {
            text: {
                switch (page.s.state) {
                case "paused":
                    if (page.s.rebuild_pending)
                        return page.s.reason === "battery" ? qsTr("重建会在接上电源后开始，或点「本次继续」。在那之前现有索引照常可搜。")
                            : qsTr("重建会在继续索引后开始。在那之前现有索引照常可搜。")
                    return (page.s.total > 0 ? qsTr("%1 / %2 个文件。").arg(page.number(page.s.done)).arg(page.number(page.s.total)) : "")
                        + (page.s.reason === "battery" ? qsTr("接上电源后自动继续，或在「性能」里关闭暂停。") : qsTr("点「继续」接着索引。"))
                case "indexing":
                    return qsTr("%1 / %2 个文件 · %3%").arg(page.number(page.s.done)).arg(page.number(page.s.total)).arg(Math.floor(page.fraction * 100))
                        + (page.s.rebuilding ? qsTr(" · 重建期间搜索结果不完整") : "")
                case "scanning":
                    return qsTr("对比目录和索引，找出新增、修改和删除的文件。")
                default: {
                    const files = (page.status.doc_files ?? 0) + (page.status.image_files ?? 0)
                    const update = page.settings.saved["update.watch"] ? qsTr("实时监听在运行。") : qsTr("实时监听已关闭。")
                    return qsTr("%1 个文件已索引；").arg(page.number(files)) + update
                }
                }
            }
        }
    }

    Notice {
        Layout.fillWidth: true
        visible: page.s.bulk === true && (page.s.images ?? 0) >= 100 && page.settings.saved["resources.profile"] !== "heavy"
            && ["indexing", "scanning", "paused"].includes(page.s.state)
        text: qsTr("队列剩余 %1 张图片").arg(page.number(page.s.images))
        actions: Kirigami.Action {
            text: qsTr("性能")
            icon.name: "go-next"
            onTriggered: page.pageRequested("resource")
        }
    }

    GridLayout {
        Layout.fillWidth: true
        columns: 3
        columnSpacing: Kirigami.Units.largeSpacing
        rowSpacing: Kirigami.Units.largeSpacing

        Repeater {
            model: [
                [page.number(page.formatCount(e => !page.imageExtensions.includes(e) && e !== "docx")), qsTr("文本、Markdown、代码")],
                [page.number(page.formatCount(e => e === "docx")), qsTr("Word（.docx）")],
                [page.number(page.status.image_files), qsTr("图片")],
                [page.number((page.status.doc_chunks ?? 0) + (page.status.image_chunks ?? 0)), qsTr("片段（向量 768 维）")],
                [page.bytes(page.status.disk_bytes), qsTr("索引占用")],
                ["0", qsTr("PDF · Excel · PPT · .doc（M5）")],
            ]

            delegate: ColumnLayout {
                required property var modelData
                required property int index

                Layout.fillWidth: true
                spacing: 0
                opacity: index === 5 ? 0.5 : 1

                QQC2.Label {
                    textFormat: Text.PlainText
                    text: parent.modelData[0]
                    font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.4
                    font.bold: true
                }
                Hint {
                    text: parent.modelData[1]
                }
            }
        }
    }
    Hint {
        readonly property var formats: page.status.formats ?? {}

        visible: Object.keys(formats).length > 0
        text: qsTr("按格式：%1").arg(Object.keys(formats).sort((a, b) => formats[b] - formats[a])
            .map(ext => `${ext || qsTr("无扩展名")} ${page.number(formats[ext])}`).join(" · "))
    }

    RowLayout {
        Layout.fillWidth: true
        Layout.topMargin: Kirigami.Units.largeSpacing

        QQC2.Label {
            textFormat: Text.PlainText
            font.bold: true
            text: qsTr("处理失败的文件（%1）").arg(page.number(page.status.errors))
        }
        Item {
            Layout.fillWidth: true
        }
        QQC2.Button {
            objectName: "retryAll"
            text: qsTr("全部重试")
            icon.name: "view-refresh"
            enabled: page.writer && page.settings.failedFiles.length > 0
            onClicked: {
                page.daemon.retry([])
                page.queued = page.settings.failedFiles.map(f => f.path)
            }
        }
    }
    Card {
        spacing: Kirigami.Units.smallSpacing

        Hint {
            visible: page.settings.failedFiles.length === 0
            text: qsTr("没有失败的文件")
        }
        Repeater {
            // The page scrolls; a few hundred rows stay cheap.
            model: page.settings.failedFiles.slice(0, 200)

            delegate: RowLayout {
                id: failedRow

                required property var modelData

                Layout.fillWidth: true
                spacing: Kirigami.Units.largeSpacing

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0

                    QQC2.Label {
                        Layout.fillWidth: true
                        textFormat: Text.PlainText
                        text: failedRow.modelData.path
                        font.family: "monospace"
                        font.pointSize: Look.smallFont.pointSize
                        wrapMode: Text.WrapAnywhere
                    }
                    Hint {
                        text: `${failedRow.modelData.error} · ${page.when(failedRow.modelData.time)}`
                        maximumLineCount: 3
                        elide: Text.ElideRight
                    }
                }
                QQC2.Button {
                    readonly property bool queued: page.queued.includes(failedRow.modelData.path)

                    text: queued ? qsTr("已排队") : qsTr("重试")
                    enabled: page.writer && !queued
                    onClicked: {
                        page.daemon.retry([failedRow.modelData.path])
                        page.queued = page.queued.concat([failedRow.modelData.path])
                    }
                }
            }
        }
        Hint {
            visible: page.settings.failedFiles.length > 200
            text: qsTr("只列出最近的 200 个。")
        }
    }

    RowLayout {
        Layout.topMargin: Kirigami.Units.largeSpacing
        visible: page.writer && !page.confirmRebuild
        spacing: Kirigami.Units.largeSpacing

        QQC2.Button {
            objectName: "rebuild"
            text: qsTr("重建索引…")
            icon.name: "view-refresh"
            onClicked: page.confirmRebuild = true
        }
        Hint {
            text: qsTr("立即重建所有索引")
        }
    }
    Notice {
        Layout.fillWidth: true
        visible: page.confirmRebuild
        type: Kirigami.MessageType.Warning
        text: {
            let text = qsTr("重建将清空现有索引，文件较多时耗时较长")
            if (page.status.on_battery && page.status.pause_on_battery && !page.status.battery_override)
                text += "<br>" + qsTr("当前为电池供电：等待电源中，点击「本次继续」后将立即开始。")
            if (page.unavailable.length > 0)
                text += "<br>" + qsTr("暂时不可用的目录（%1）无法被读取：需要等待目录重新可用").arg(Markup.escape(page.unavailable.join("、")))
            return text
        }
        actions: [
            Kirigami.Action {
                objectName: "confirmRebuild"
                text: qsTr("确认重建")
                icon.name: "view-refresh"
                onTriggered: {
                    page.daemon.control("Rebuild")
                    page.confirmRebuild = false
                }
            },
            Kirigami.Action {
                text: qsTr("取消")
                icon.name: "dialog-cancel"
                onTriggered: page.confirmRebuild = false
            },
        ]
    }
}
