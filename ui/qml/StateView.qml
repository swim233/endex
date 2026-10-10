pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

// Everything that is not a result list: tips for an empty query, no results, no daemon,
// nothing indexed yet, an index being rebuilt, an unusable index.
Item {
    id: stateView

    required property string view
    required property string query
    required property string mode
    required property Daemon daemon

    signal modeRequested(string id)
    signal copy(string text)

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
                title: qsTr("还没有索引"),
                text: qsTr("在终端里列出要索引的文件并建立索引，完成后就能搜索。设置窗口（M3）会带你选目录。"),
                command: "find ~/Documents -type f > ~/endex-files.txt\nendex index --manifest ~/endex-files.txt",
            }
        case "broken":
            return {
                icon: "data-error",
                title: qsTr("索引暂时不能用"),
                text: qsTr("如果换过嵌入模型或前缀风格，需要用原来的清单重建索引。"),
                error: daemon.error,
                command: "endex index --rebuild --manifest <清单文件>",
            }
        case "rebuilding":
            return {
                icon: "view-refresh",
                title: qsTr("正在重建索引…"),
                text: qsTr("endex index 正在重建索引，完成后这里会自动刷新。"),
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

        Rectangle {
            Layout.fillWidth: true
            visible: !!stateView.content.command
            implicitHeight: commandRow.implicitHeight + Kirigami.Units.largeSpacing * 2
            radius: Kirigami.Units.cornerRadius
            color: Kirigami.Theme.alternateBackgroundColor
            border.color: Kirigami.ColorUtils.linearInterpolation(Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, 0.2)

            RowLayout {
                id: commandRow

                anchors.fill: parent
                anchors.margins: Kirigami.Units.largeSpacing

                QQC2.Label {
                    textFormat: Text.PlainText
                    Layout.fillWidth: true
                    text: stateView.content.command ?? ""
                    font.family: "monospace"
                    wrapMode: Text.WrapAnywhere
                }
                QQC2.ToolButton {
                    icon.name: "edit-copy"
                    display: QQC2.AbstractButton.IconOnly
                    text: qsTr("复制命令")
                    focusPolicy: Qt.NoFocus
                    onClicked: stateView.copy(stateView.content.command)
                    QQC2.ToolTip.text: text
                    QQC2.ToolTip.visible: hovered
                }
            }
        }

        GridLayout {
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: Kirigami.Units.largeSpacing
            visible: stateView.view === "tips"
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
