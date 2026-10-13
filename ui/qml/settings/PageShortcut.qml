pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

// 快捷键指引: how to bind `endex toggle` in System Settings by hand, and the popup's keys.
ColumnLayout {
    id: page

    required property Popup popup

    signal copy(string text)

    readonly property string command: popup.toggleCommand()
    readonly property var steps: [
        qsTr("打开<b>系统设置 › 键盘 › 快捷键</b>。"),
        qsTr("点<b>添加新项 › 命令或脚本</b>。"),
        qsTr("命令填下面这一行（完整路径，不依赖 PATH），名称随意，例如「Endex」。"),
        qsTr("设置快捷键。Plasma 默认把 Alt+Space 给了 KRunner：可以换一个组合，例如 Meta+Space；或者先在<b>快捷键 › KRunner ›「激活 KRunner」</b>里去掉 Alt+Space（KRunner 仍可用 Alt+F2），再把 Alt+Space 给 Endex。"),
        qsTr("点「应用」，然后按一下这个快捷键试试。"),
    ]
    readonly property var keys: [
        [qsTr("唤出 / 隐藏浮窗"), qsTr("你绑定的快捷键"), qsTr("运行 endex toggle")],
        [qsTr("关闭浮窗"), "Esc", qsTr("先关菜单，再关浮窗")],
        [qsTr("选择结果"), "↑ ↓ ← →", qsTr("←→ 在图片网格里移动")],
        [qsTr("在文档和图片之间跳转"), "Tab", ""],
        [qsTr("翻页"), "PgUp / PgDn", qsTr("文档和图片各自翻页")],
        [qsTr("打开"), "Enter", qsTr("空查询时重新搜索选中的最近搜索")],
        [qsTr("在 Dolphin 中显示"), "Ctrl+Enter", ""],
        [qsTr("复制路径"), "Ctrl+C", qsTr("搜索框里有选中文字时，仍是普通复制")],
        [qsTr("复制片段"), "Ctrl+Shift+C", qsTr("图片复制 OCR 文字")],
        [qsTr("切换检索模式"), "Ctrl+1 / 2 / 3", qsTr("混合 / 向量 / 关键词")],
        [qsTr("打开设置"), "Ctrl+,", ""],
        [qsTr("找相似（M4）"), "Ctrl+S", ""],
        [qsTr("重排（M4）"), "Ctrl+R", ""],
    ]

    spacing: Kirigami.Units.largeSpacing

    Notice {
        Layout.fillWidth: true
        visible: true
        text: qsTr("<b>手动设置</b>：请按下面的步骤在系统设置里绑定。")
    }

    Repeater {
        model: page.steps

        delegate: RowLayout {
            id: step

            required property string modelData
            required property int index

            Layout.fillWidth: true
            spacing: Kirigami.Units.largeSpacing

            Rectangle {
                Layout.alignment: Qt.AlignTop
                implicitWidth: Kirigami.Units.gridUnit * 1.4
                implicitHeight: implicitWidth
                radius: width / 2
                color: Kirigami.Theme.highlightColor

                QQC2.Label {
                    anchors.centerIn: parent
                    textFormat: Text.PlainText
                    text: step.index + 1
                    color: Kirigami.Theme.highlightedTextColor
                    font.bold: true
                }
            }
            QQC2.Label {
                Layout.fillWidth: true
                textFormat: Text.StyledText
                text: step.modelData
                wrapMode: Text.Wrap
            }
        }
    }

    CommandBox {
        Layout.fillWidth: true
        command: page.command
        onCopy: text => page.copy(text)
    }
    Hint {
        text: qsTr("Wayland 下应用没法可靠地抢全局快捷键，所以交给 KDE 绑定，Endex 只提供 endex toggle 命令。")
    }

    QQC2.Label {
        Layout.topMargin: Kirigami.Units.largeSpacing
        textFormat: Text.PlainText
        font.bold: true
        text: qsTr("应用里的快捷键")
    }
    Card {
        spacing: Kirigami.Units.smallSpacing

        GridLayout {
            Layout.fillWidth: true
            columns: 3
            columnSpacing: Kirigami.Units.largeSpacing * 2
            rowSpacing: Kirigami.Units.smallSpacing

            Repeater {
                model: page.keys

                QQC2.Label {
                    required property var modelData
                    required property int index

                    Layout.row: index
                    Layout.column: 0
                    textFormat: Text.PlainText
                    text: modelData[0]
                    color: index >= page.keys.length - 2 ? Kirigami.Theme.disabledTextColor : Kirigami.Theme.textColor
                }
            }
            Repeater {
                model: page.keys

                KeyCap {
                    required property var modelData
                    required property int index

                    Layout.row: index
                    Layout.column: 1
                    text: modelData[1]
                    opacity: index >= page.keys.length - 2 ? 0.5 : 1
                }
            }
            Repeater {
                model: page.keys

                Hint {
                    required property var modelData
                    required property int index

                    Layout.row: index
                    Layout.column: 2
                    text: modelData[2]
                }
            }
        }
    }
}
