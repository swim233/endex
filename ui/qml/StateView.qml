pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

// Everything that is not a result list: recent searches and tips for an empty query, no
// results, no daemon, no folders yet, an index that cannot be read now.
Item {
    id: stateView

    required property string view
    required property string query
    required property string mode
    required property Daemon daemon
    // The selected recent search (↑↓ in the empty field), -1 for none.
    property int recentIndex: -1

    signal modeRequested(string id)
    signal copy(string text)
    signal rerun(string query)
    signal settingsRequested(string page, bool welcome)

    clip: true

    readonly property var tips: [
        [["↑", "↓"], qsTr("选择结果；←→ 在图片网格里移动")],
        [["Tab"], qsTr("在「文档」和「图片」之间切换")],
        [["PgUp", "PgDn"], qsTr("翻页（文档和图片各自翻页）")],
        [["Ctrl+1", "2", "3"], qsTr("切换 混合 / 向量 / 关键词")],
        [["↵", "Ctrl+↵"], qsTr("打开 / 在 Dolphin 中显示")],
        [["Ctrl+C", "Ctrl+⇧+C"], qsTr("复制路径 / 复制片段")],
        [["Esc"], qsTr("关闭浮窗（再按一次快捷键也可以）")],
    ]

    readonly property var content: {
        switch (view) {
        case "tips":
            return {
                icon: "search",
                title: qsTr("搜索文档和图片"),
                text: qsTr("截图里的文字也能搜到；用一句话描述画面，向量模式会按语义找图片。"),
            }
        case "none":
            return {
                icon: "edit-find",
                title: qsTr("没有找到「%1」").arg(query),
                text: mode === "keyword" ? qsTr("关键词模式只匹配文字，不识别图片内容。") : qsTr("换个说法试试。"),
            }
        case "failed":
            return {
                icon: "dialog-error",
                title: qsTr("搜索出错"),
                text: qsTr("这次搜索没有完成。换个模式试试；如果一直这样，可能需要重建索引。"),
                error: daemon.searchError,
            }
        case "down":
            return {
                icon: "system-run",
                title: qsTr("Endex 守护进程没有运行"),
                text: qsTr("浮窗连不上 D-Bus 服务 org.endex.Daemon1，现在没法搜索。"),
                error: daemon.error,
                command: "systemctl --user start endexd",
            }
        case "first":
            return {
                icon: "folder-new",
                title: qsTr("还没有要索引的目录"),
                text: qsTr("Endex 不会自动扫描「图片」和「文档」，先选好要索引的目录。"),
            }
        case "empty":
            return {
                icon: "folder-open",
                title: qsTr("还没有索引"),
                text: qsTr("守护进程以只读方式提供 %1，那里还没有建好的索引。").arg(daemon.status.index_path ?? ""),
            }
        case "broken":
            return {
                icon: "data-error",
                title: qsTr("索引暂时不能用"),
                text: qsTr("如果换过嵌入模型，需要在设置的「索引状态」里重建索引。"),
                error: daemon.error,
            }
        case "rebuilding":
            return {
                icon: "view-refresh",
                title: qsTr("正在重建索引…"),
                text: qsTr("索引正在被替换，完成后这里会自动刷新。"),
            }
        default:
            return {
                icon: "network-connect",
                title: qsTr("正在连接守护进程…"),
                text: "",
            }
        }
    }

    ColumnLayout {
        anchors.centerIn: parent
        width: Math.min(parent.width - Kirigami.Units.gridUnit * 4, Kirigami.Units.gridUnit * 34)
        spacing: Kirigami.Units.largeSpacing

        Kirigami.Icon {
            Layout.alignment: Qt.AlignHCenter
            implicitWidth: Kirigami.Units.iconSizes.huge
            implicitHeight: implicitWidth
            source: stateView.content.icon
            color: Kirigami.Theme.disabledTextColor
        }

        Kirigami.Heading {
            textFormat: Text.PlainText
            Layout.fillWidth: true
            level: 2
            text: stateView.content.title
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
        }

        QQC2.Label {
            textFormat: Text.PlainText
            Layout.fillWidth: true
            visible: text !== ""
            text: stateView.content.text
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            color: Kirigami.Theme.disabledTextColor
        }

        QQC2.BusyIndicator {
            Layout.alignment: Qt.AlignHCenter
            visible: stateView.view === "connecting" || stateView.view === "rebuilding"
            running: visible
        }

        QQC2.Label {
            textFormat: Text.PlainText
            Layout.fillWidth: true
            // Library errors can run to dozens of lines of nested causes.
            maximumLineCount: 3
            elide: Text.ElideRight
            visible: !!stateView.content.error
            text: stateView.content.error ?? ""
            font.family: "monospace"
            font.pointSize: Kirigami.Theme.smallFont.pointSize
            color: Kirigami.Theme.negativeTextColor
            wrapMode: Text.Wrap
            horizontalAlignment: Text.AlignHCenter
        }

        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            spacing: Kirigami.Units.largeSpacing

            QQC2.Button {
                visible: stateView.view === "down"
                text: stateView.daemon.state === Daemon.Starting ? qsTr("正在启动…") : qsTr("启动")
                icon.name: "media-playback-start"
                enabled: stateView.daemon.state !== Daemon.Starting
                focusPolicy: Qt.NoFocus
                onClicked: stateView.daemon.start()
            }
            QQC2.Button {
                objectName: "firstRunSettings"
                visible: stateView.view === "first"
                text: qsTr("打开设置 › 索引目录")
                icon.name: "configure"
                highlighted: true
                Accessible.defaultButton: true
                focusPolicy: Qt.NoFocus
                onClicked: stateView.settingsRequested("dirs", true)
            }
            QQC2.Button {
                visible: stateView.view === "broken" && stateView.daemon.status.mode === "writer"
                text: qsTr("打开设置 › 索引状态")
                icon.name: "configure"
                focusPolicy: Qt.NoFocus
                onClicked: stateView.settingsRequested("status", false)
            }
            QQC2.Button {
                visible: (stateView.view === "none" || stateView.view === "failed") && stateView.mode !== "vector"
                text: qsTr("改用向量模式（按语义找）")
                focusPolicy: Qt.NoFocus
                onClicked: stateView.modeRequested("vector")
            }
            QQC2.Button {
                visible: (stateView.view === "none" || stateView.view === "failed") && stateView.mode !== "hybrid"
                text: qsTr("改用混合")
                focusPolicy: Qt.NoFocus
                onClicked: stateView.modeRequested("hybrid")
            }
        }

        CommandBox {
            Layout.fillWidth: true
            visible: !!stateView.content.command
            command: stateView.content.command ?? ""
            onCopy: text => stateView.copy(text)
        }

        ColumnLayout {
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: Kirigami.Units.largeSpacing
            visible: stateView.view === "first"
            spacing: Kirigami.Units.smallSpacing

            Repeater {
                model: [
                    qsTr("在设置里添加要索引的目录，例如 ~/Notes"),
                    qsTr("点「应用」，等首次索引完成（之后只做增量）"),
                    qsTr("按你绑定的快捷键开始搜索"),
                ]

                delegate: RowLayout {
                    id: step

                    required property string modelData
                    required property int index

                    spacing: Kirigami.Units.largeSpacing

                    KeyCap {
                        text: step.index + 1
                    }
                    QQC2.Label {
                        textFormat: Text.PlainText
                        text: step.modelData
                        color: step.index === 0 ? Kirigami.Theme.textColor : Kirigami.Theme.disabledTextColor
                    }
                }
            }
        }

        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: Kirigami.Units.largeSpacing
            visible: stateView.view === "tips"
            spacing: Kirigami.Units.gridUnit * 2

            ColumnLayout {
                Layout.alignment: Qt.AlignTop
                Layout.preferredWidth: Kirigami.Units.gridUnit * 12
                visible: stateView.daemon.recent.length > 0
                spacing: 0

                QQC2.Label {
                    textFormat: Text.PlainText
                    text: qsTr("最近搜索")
                    font.bold: true
                    color: Kirigami.Theme.disabledTextColor
                }
                Repeater {
                    model: stateView.daemon.recent

                    QQC2.ItemDelegate {
                        id: recentRow

                        required property string modelData
                        required property int index

                        Layout.fillWidth: true
                        text: modelData
                        highlighted: index === stateView.recentIndex
                        focusPolicy: Qt.NoFocus
                        onClicked: stateView.rerun(modelData)

                        // Queries are user text: never rich text.
                        contentItem: RowLayout {
                            spacing: Kirigami.Units.smallSpacing

                            Kirigami.Icon {
                                source: "chronometer"
                                implicitWidth: Kirigami.Units.iconSizes.small
                                implicitHeight: implicitWidth
                            }
                            QQC2.Label {
                                Layout.fillWidth: true
                                textFormat: Text.PlainText
                                text: recentRow.modelData
                                elide: Text.ElideRight
                                color: recentRow.highlighted ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor
                            }
                        }
                    }
                }
            }

            GridLayout {
                Layout.alignment: Qt.AlignTop
                columns: 2
                columnSpacing: Kirigami.Units.largeSpacing * 2
                rowSpacing: Kirigami.Units.smallSpacing

                Repeater {
                    model: stateView.tips

                    delegate: RowLayout {
                        id: keys

                        required property var modelData
                        required property int index

                        Layout.row: index
                        Layout.column: 0
                        Layout.alignment: Qt.AlignRight

                        Repeater {
                            model: keys.modelData[0]

                            KeyCap {
                                required property string modelData

                                text: modelData
                            }
                        }
                    }
                }

                Repeater {
                    model: stateView.tips

                    QQC2.Label {
                        required property var modelData
                        required property int index

                        textFormat: Text.PlainText
                        Layout.row: index
                        Layout.column: 1
                        text: modelData[1]
                    }
                }
            }
        }

        QQC2.Label {
            textFormat: Text.PlainText
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: Kirigami.Units.largeSpacing
            visible: stateView.view === "tips" && stateView.daemon.state === Daemon.Ready
            text: qsTr("已索引 %1 个文件：文档 %2 · 图片 %3")
                .arg((stateView.daemon.status.doc_files ?? 0) + (stateView.daemon.status.image_files ?? 0))
                .arg(stateView.daemon.status.doc_files ?? 0).arg(stateView.daemon.status.image_files ?? 0)
            font: Kirigami.Theme.smallFont
            color: Kirigami.Theme.disabledTextColor
        }
    }
}
