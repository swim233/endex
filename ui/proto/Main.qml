import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Window {
    id: root

    required property QtObject popup
    readonly property string searchText: field.text
    readonly property string preeditText: field.preeditText

    // Called by Popup::Show after the window is mapped.
    function summoned(query) {
        if (query)
            field.text = query
        field.forceActiveFocus()
        field.selectAll()
    }

    width: 1100
    height: 640
    color: "transparent"
    title: qsTr("Endex")

    Shortcut {
        sequences: [StandardKey.Cancel]
        onActivated: root.popup.hide()
    }

    Shortcut {
        sequence: "Ctrl+Shift+C"
        onActivated: copyButton.clicked()
    }

    Rectangle {
        anchors.fill: parent
        radius: 12
        color: root.palette.window
        border.color: root.palette.mid

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 20
            spacing: 14

            TextField {
                id: field

                objectName: "searchField"
                Layout.fillWidth: true
                font.pixelSize: 22
                placeholderText: qsTr("搜索文档和图片…")
            }

            RowLayout {
                spacing: 12

                Button {
                    id: copyButton

                    text: qsTr("复制输入内容  Ctrl+Shift+C")
                    focusPolicy: Qt.NoFocus
                    onClicked: {
                        root.popup.copy(field.text)
                        copied.text = qsTr("已复制：%1").arg(field.text)
                    }
                }

                Label {
                    id: copied
                }
            }

            GridLayout {
                columns: 2
                columnSpacing: 16

                Label { text: qsTr("搜索框焦点") }
                Label { text: field.activeFocus ? qsTr("有 ✓") : qsTr("无 ✗") }
                Label { text: qsTr("窗口激活") }
                Label { text: root.active ? qsTr("是 ✓") : qsTr("否 ✗") }
                Label { text: qsTr("输入法预编辑") }
                Label { text: field.preeditText ? field.preeditText : "—" }
            }

            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: root.palette.placeholderText
                text: qsTr("Phase 0 原型。Esc 关闭；再次按快捷键（或 endex toggle）切换显示。")
            }

            Item { Layout.fillHeight: true }
        }
    }
}
