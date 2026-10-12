pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

// 增量与监听: periodic scans and inotify, and what each combination means.
ColumnLayout {
    id: page

    required property Daemon daemon
    required property Settings settings

    readonly property bool scan: settings.draft["update.scan"] === true
    readonly property bool watch: settings.draft["update.watch"] === true
    readonly property var intervals: [["startup", qsTr("仅在守护进程启动时")], ["hourly", qsTr("每小时")], ["daily", qsTr("每天一次")]]
    // 0 both, 1 scan only, 2 watch only, 3 neither (the table's rows).
    readonly property int combination: scan && watch ? 0 : scan ? 1 : watch ? 2 : 3
    readonly property var rows: [
        [qsTr("都开"), qsTr("启动时补齐离线期间的变化，运行中实时更新。推荐。")],
        [qsTr("只开扫描"), qsTr("改动要等下一次扫描才能搜到；不占用 inotify watch。")],
        [qsTr("只开监听"), qsTr("运行中实时更新，但守护进程停止期间的改动会漏掉。")],
        [qsTr("都关"), qsTr("索引不再自动更新，只能手动「立即扫描」或「重建索引」。")],
    ]

    function number(n) {
        return Number(n ?? 0).toLocaleString(Qt.locale(), "f", 0)
    }

    spacing: Kirigami.Units.largeSpacing

    GridLayout {
        Layout.fillWidth: true
        columns: page.width > Kirigami.Units.gridUnit * 30 ? 2 : 1
        columnSpacing: Kirigami.Units.largeSpacing
        rowSpacing: Kirigami.Units.largeSpacing

        Card {
            Layout.alignment: Qt.AlignTop

            SettingSwitch {
                Layout.fillWidth: true
                settings: page.settings
                key: "update.scan"
                text: qsTr("增量扫描")
                hint: qsTr("对比目录和索引，只处理新增、修改、删除的文件。")
            }
            RowLayout {
                visible: page.scan

                Hint {
                    Layout.fillWidth: false
                    text: qsTr("触发时机")
                }
                SettingCombo {
                    settings: page.settings
                    key: "update.scan_interval"
                    options: page.intervals
                }
            }
        }

        Card {
            Layout.alignment: Qt.AlignTop

            SettingSwitch {
                Layout.fillWidth: true
                settings: page.settings
                key: "update.watch"
                text: qsTr("实时监听（inotify）")
                hint: qsTr("文件一变化就更新索引，通常几秒内生效。")
            }
            ColumnLayout {
                readonly property var status: page.daemon.status

                Layout.fillWidth: true
                visible: page.watch && status.watch !== "off" && status.watch !== undefined
                spacing: Kirigami.Units.smallSpacing

                Hint {
                    text: qsTr("已占用 %1 / %2 个 watch（fs.inotify.max_user_watches）")
                        .arg(page.number(parent.status.watches)).arg(page.number(parent.status.max_watches))
                }
                QQC2.ProgressBar {
                    Layout.fillWidth: true
                    from: 0
                    to: Math.max(1, parent.status.max_watches ?? 1)
                    value: parent.status.watches ?? 0
                }
                Hint {
                    visible: parent.status.watch === "limit"
                    color: Kirigami.Theme.neutralTextColor
                    text: qsTr("watch 数量到了系统上限，已改为至少每小时扫描一次。可以调大 fs.inotify.max_user_watches。")
                }
                Hint {
                    visible: parent.status.watch === "error"
                    color: Kirigami.Theme.negativeTextColor
                    text: qsTr("监听没能启动，改动要等扫描才能发现。")
                }
            }
            Hint {
                visible: !page.watch
                text: qsTr("关闭后不占用 inotify watch。")
            }
        }
    }

    Kirigami.InlineMessage {
        Layout.fillWidth: true
        visible: page.combination === 3
        type: Kirigami.MessageType.Warning
        text: qsTr("两项都关了：索引不会再自动更新。")
    }
    Kirigami.InlineMessage {
        Layout.fillWidth: true
        visible: page.combination === 2
        type: Kirigami.MessageType.Warning
        text: qsTr("只开监听时，守护进程没运行期间发生的改动会被漏掉。")
    }
    Kirigami.InlineMessage {
        Layout.fillWidth: true
        visible: page.combination === 1
        type: Kirigami.MessageType.Warning
        text: {
            switch (page.settings.draft["update.scan_interval"]) {
            case "hourly":
                return qsTr("实时监听关了：改动要等下一次扫描（每小时一次）或守护进程重启才会被发现。")
            case "daily":
                return qsTr("实时监听关了：改动要等每天一次的扫描或守护进程重启才会被发现。")
            default:
                return qsTr("实时监听关了：改动只在守护进程启动时才会被发现。")
            }
        }
    }

    Card {
        spacing: 0

        Repeater {
            model: page.rows

            delegate: Rectangle {
                id: comboRow

                required property var modelData
                required property int index
                readonly property bool current: index === page.combination

                Layout.fillWidth: true
                implicitHeight: comboLine.implicitHeight + Kirigami.Units.largeSpacing
                radius: Kirigami.Units.cornerRadius
                color: current ? Kirigami.Theme.highlightColor : "transparent"
                opacity: current ? 1 : 0.9

                RowLayout {
                    id: comboLine

                    anchors.fill: parent
                    anchors.leftMargin: Kirigami.Units.smallSpacing
                    anchors.rightMargin: Kirigami.Units.smallSpacing
                    spacing: Kirigami.Units.largeSpacing

                    QQC2.Label {
                        Layout.preferredWidth: Kirigami.Units.gridUnit * 5
                        textFormat: Text.PlainText
                        text: comboRow.modelData[0]
                        font.bold: comboRow.current
                        color: comboRow.current ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor
                    }
                    QQC2.Label {
                        Layout.fillWidth: true
                        textFormat: Text.PlainText
                        text: comboRow.modelData[1]
                        wrapMode: Text.Wrap
                        color: comboRow.current ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor
                    }
                    QQC2.Label {
                        visible: comboRow.current
                        textFormat: Text.PlainText
                        text: qsTr("当前")
                        font: Kirigami.Theme.smallFont
                        color: Kirigami.Theme.highlightedTextColor
                    }
                }
            }
        }
    }

    RowLayout {
        spacing: Kirigami.Units.largeSpacing

        QQC2.Button {
            objectName: "rescan"
            text: qsTr("立即扫描")
            icon.name: "view-refresh"
            enabled: page.daemon.status.mode === "writer"
            onClicked: page.daemon.control("Rescan")
        }
        Hint {
            text: qsTr("马上对比所有目录和索引，补上漏掉的改动（不会重新嵌入没变的文件）。")
        }
    }
}
