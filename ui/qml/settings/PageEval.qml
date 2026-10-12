pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex
import "Markup.js" as Markup

// 评测与日志: the public eval sets (run in the background, read-only) and the usage log.
ColumnLayout {
    id: page

    required property Daemon daemon
    required property Settings settings
    required property EvalRun evalRun
    // A read-only daemon keeps no usage log.
    required property bool writable

    readonly property string config: daemon.status.config_path ?? ""
    readonly property string dataDir: daemon.status.data_dir ?? "<data_dir>"
    readonly property var sets: Array.from(evalRun.results.sets ?? [])
    readonly property var names: ({
        "xm3600/queries": qsTr("XM3600 中文照片"),
        "xm3600/queries.en": qsTr("XM3600 英文照片"),
        "miracl_vision/queries": qsTr("MIRACL-VISION 页面图"),
        "t2retrieval/queries": qsTr("T2Retrieval 中文段落"),
        "scifact/queries": qsTr("SciFact 英文摘要"),
    })
    property bool logOpen
    property bool confirmClear

    // The table cells of one set: name, kind, then recall and MRR per mode, the row's best marked.
    function row(set) {
        const modes = ["vector", "keyword", "hybrid"].map(mode => set.modes.find(m => m.mode === mode) ?? { recall: 0, mrr: 0 })
        const bestRecall = Math.max(...modes.map(m => m.recall))
        const bestMrr = Math.max(...modes.map(m => m.mrr))
        return [
            { text: names[`${set.set}/${set.queries}`] ?? `${set.set} · ${set.queries}` },
            { text: set.kind === "image" ? qsTr("图片") : qsTr("文本") },
        ].concat(modes.map(m => ({
            recall: m.recall,
            mrr: m.mrr,
            bestRecall: m.recall === bestRecall && m.recall > 0,
            bestMrr: m.mrr === bestMrr && m.mrr > 0,
        })))
    }

    Component.onCompleted: {
        if (config !== "")
            evalRun.load(config)
    }

    spacing: Kirigami.Units.largeSpacing

    RowLayout {
        Layout.fillWidth: true
        spacing: Kirigami.Units.largeSpacing

        QQC2.Label {
            textFormat: Text.PlainText
            font.bold: true
            text: qsTr("最近一次评测")
        }
        Hint {
            Layout.fillWidth: false
            text: page.evalRun.results.time
                ? Qt.formatDateTime(new Date(page.evalRun.results.time * 1000), "yyyy-MM-dd HH:mm")
                : qsTr("还没有运行过")
        }
        Item {
            Layout.fillWidth: true
        }
        QQC2.Button {
            objectName: "runEval"
            text: page.evalRun.running ? qsTr("停止") : qsTr("运行评测")
            icon.name: page.evalRun.running ? "media-playback-stop" : "media-playback-start"
            highlighted: !page.evalRun.running
            enabled: page.config !== ""
            onClicked: page.evalRun.running ? page.evalRun.stop() : page.evalRun.run(page.config)
        }
    }
    ColumnLayout {
        Layout.fillWidth: true
        visible: page.evalRun.running
        spacing: Kirigami.Units.smallSpacing

        QQC2.ProgressBar {
            Layout.fillWidth: true
            indeterminate: true
        }
        Hint {
            text: qsTr("正在评测 %1 … 在后台用查询设备跑，约一两分钟；索引和搜索照常进行。").arg(page.evalRun.progress)
        }
    }
    Kirigami.InlineMessage {
        Layout.fillWidth: true
        visible: page.evalRun.error !== ""
        type: Kirigami.MessageType.Error
        text: qsTr("<b>评测没有完成</b>：%1").arg(Markup.escape(page.evalRun.error))
    }

    Card {
        visible: page.sets.length > 0
        spacing: Kirigami.Units.smallSpacing

        GridLayout {
            Layout.fillWidth: true
            columns: 5
            columnSpacing: Kirigami.Units.largeSpacing * 2
            rowSpacing: Kirigami.Units.smallSpacing

            Repeater {
                model: [qsTr("评测集"), qsTr("类型"), qsTr("向量"), qsTr("关键词"), qsTr("混合")]

                QQC2.Label {
                    required property string modelData
                    required property int index

                    Layout.fillWidth: index === 0
                    textFormat: Text.PlainText
                    text: modelData
                    font.bold: true
                }
            }
            Repeater {
                // QML's JavaScript has no flatMap.
                model: page.sets.reduce((cells, set) => cells.concat(page.row(set)), [])

                QQC2.Label {
                    required property var modelData

                    textFormat: Text.StyledText
                    font.family: modelData.text === undefined ? "monospace" : Kirigami.Theme.defaultFont.family
                    text: {
                        const d = modelData
                        if (d.text !== undefined)
                            return Markup.escape(d.text)
                        const number = (v, best) => best ? `<b>${v.toFixed(3)}</b>` : v.toFixed(3)
                        return `${number(d.recall, d.bestRecall)} / ${number(d.mrr, d.bestMrr)}`
                    }
                }
            }
        }
    }
    Hint {
        text: page.sets.length > 0
            ? qsTr("每格是 R@%1 / MRR，加粗是该行最高；每集 %2 条查询，各用独立索引。评测只读取 %3/eval 下已准备好的公开评测集（just eval），不会改动它们；结果存在 %3/eval-results/last.json。")
                .arg(page.evalRun.results.limit ?? 10).arg(page.sets[0].count).arg(page.dataDir)
            : qsTr("评测读取 %1/eval 下已准备好的公开评测集（用 just eval 准备），只读，不会改动它们。").arg(page.dataDir)
    }

    Kirigami.FormLayout {
        Layout.fillWidth: true
        Layout.topMargin: Kirigami.Units.largeSpacing
        enabled: page.writable

        Kirigami.Separator {
            Kirigami.FormData.isSection: true
            Kirigami.FormData.label: qsTr("使用日志")
        }
        Hint {
            Kirigami.FormData.isSection: true
            visible: !page.writable
            text: qsTr("只读模式下不可用：只读的守护进程不记录使用日志。")
        }
        SettingSwitch {
            Kirigami.FormData.label: qsTr("记录：")
            Layout.fillWidth: true
            settings: page.settings
            key: "log.usage"
            text: qsTr("记录使用日志")
            hint: qsTr("只存在本机 %1/usage.jsonl：查询词、检索模式、结果数、耗时和打开的文件路径。不记录文件内容，不联网。浮窗的「最近搜索」来自这里。").arg(page.dataDir)
        }
        RowLayout {
            spacing: Kirigami.Units.largeSpacing

            QQC2.Button {
                objectName: "viewLog"
                text: page.logOpen ? qsTr("收起") : qsTr("查看日志")
                icon.name: "view-list-text"
                onClicked: {
                    page.logOpen = !page.logOpen
                    if (page.logOpen)
                        page.settings.fetchUsageLog()
                }
            }
            QQC2.Button {
                objectName: "clearLog"
                visible: !page.confirmClear
                text: qsTr("清除日志…")
                icon.name: "edit-clear-history"
                onClicked: page.confirmClear = true
            }
            Hint {
                Layout.fillWidth: false
                visible: page.confirmClear
                text: qsTr("清除后不能恢复。")
            }
            QQC2.Button {
                objectName: "confirmClearLog"
                visible: page.confirmClear
                text: qsTr("确认清除")
                icon.name: "edit-delete"
                onClicked: {
                    page.settings.clearUsageLog()
                    page.confirmClear = false
                }
            }
            QQC2.Button {
                visible: page.confirmClear
                text: qsTr("取消")
                onClicked: page.confirmClear = false
            }
        }
    }
    QQC2.TextArea {
        objectName: "usageLog"
        Layout.fillWidth: true
        visible: page.logOpen && page.writable
        readOnly: true
        wrapMode: Text.WrapAnywhere
        font.family: "monospace"
        font.pointSize: Kirigami.Theme.smallFont.pointSize
        text: page.settings.usageLog.length > 0 ? page.settings.usageLog.join("\n") : qsTr("日志是空的。")
    }
}
