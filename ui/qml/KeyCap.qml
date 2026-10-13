import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import Endex

Rectangle {
    property alias text: label.text

    implicitWidth: label.implicitWidth + Look.smallSpacing * 2
    implicitHeight: label.implicitHeight + 2
    radius: Math.min(3, Look.radius)
    color: Kirigami.Theme.alternateBackgroundColor
    border.color: Kirigami.ColorUtils.linearInterpolation(Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, 0.3)

    QQC2.Label {
        id: label

        textFormat: Text.PlainText
        anchors.centerIn: parent
        font.family: "monospace"
        font.pointSize: Look.smallFont.pointSize * 0.95
    }
}
