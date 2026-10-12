import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// A shell command in monospace, with a button that copies it.
Rectangle {
    id: box

    required property string command

    signal copy(string text)

    implicitWidth: row.implicitWidth + Kirigami.Units.largeSpacing * 2
    implicitHeight: row.implicitHeight + Kirigami.Units.largeSpacing * 2
    radius: Kirigami.Units.cornerRadius
    color: Kirigami.Theme.alternateBackgroundColor
    border.color: Kirigami.ColorUtils.linearInterpolation(Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, 0.2)

    RowLayout {
        id: row

        anchors.fill: parent
        anchors.margins: Kirigami.Units.largeSpacing

        QQC2.Label {
            textFormat: Text.PlainText
            Layout.fillWidth: true
            text: box.command
            font.family: "monospace"
            wrapMode: Text.WrapAnywhere
        }
        QQC2.ToolButton {
            icon.name: "edit-copy"
            display: QQC2.AbstractButton.IconOnly
            text: qsTr("复制命令")
            focusPolicy: Qt.NoFocus
            onClicked: box.copy(box.command)
            QQC2.ToolTip.text: text
            QQC2.ToolTip.visible: hovered
        }
    }
}
