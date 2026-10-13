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
        [qsTr("开启实时监听与增量扫描"), qsTr("程序启动时重建离线期间的更改，运行中实时更新，会增大性能开销")],
        [qsTr("仅开启增量扫描"), qsTr("仅在每次扫描时重建更改")],
        [qsTr("仅开启实时监听"), qsTr("运行中实时更新，无法自动重建守护进程离线时的更改")],
        [qsTr("关闭实时监听与增量扫描"), qsTr("索引不再自动更新，需要手动「立即扫描」或「重建索引」。")],
    ]

    // The longest combination name sets the width of the names' column.
    TextMetrics {
        id: longestName

        font.bold: true
        text: page.rows.map(r => r[0]).reduce((a, b) => b.length > a.length ? b : a)
    }

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
                hint: qsTr("将在文件变化时实时更新索引")
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
                    text: qsTr("监听启动失败")
                }
            }
            Hint {
                visible: !page.watch
                text: qsTr("关闭后不占用 inotify watch。")
            }
        }
    }

    Notice {
        Layout.fillWidth: true
        visible: page.combination === 3
        type: Kirigami.MessageType.Warning
        text: qsTr("索引不会再自动更新")
    }
    Notice {
        Layout.fillWidth: true
        visible: page.combination === 2
        type: Kirigami.MessageType.Warning
        text: qsTr("可能遗漏部分文件改动")
    }
    Notice {
        Layout.fillWidth: true
        visible: page.combination === 1
        type: Kirigami.MessageType.Warning
        text: {
            switch (page.settings.draft["update.scan_interval"]) {
            case "hourly":
                return qsTr("将不再实时监听文件变化")
            case "daily":
                return qsTr("将不再实时监听文件变化")
            default:
                return qsTr("仅在守护进程启动时更新索引")
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
                radius: Look.radius
                color: current ? Kirigami.Theme.highlightColor : "transparent"
                opacity: current ? 1 : 0.9

                RowLayout {
                    id: comboLine

                    anchors.fill: parent
                    anchors.leftMargin: Kirigami.Units.smallSpacing
                    anchors.rightMargin: Kirigami.Units.smallSpacing
                    spacing: Kirigami.Units.largeSpacing

                    QQC2.Label {
                        Layout.preferredWidth: Math.ceil(longestName.width)
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
                        font: Look.smallFont
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
            text: qsTr("立即对所有更改进行扫描，重建更改的索引")
        }
    }
}
