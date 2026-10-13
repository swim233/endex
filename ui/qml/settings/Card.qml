import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

// A framed group of rows.
Rectangle {
    default property alias content: column.data
    property alias spacing: column.spacing

    Layout.fillWidth: true
    implicitWidth: column.implicitWidth + Look.largeSpacing * 2
    implicitHeight: column.implicitHeight + Look.largeSpacing * 2
    radius: Look.radius
    color: Kirigami.Theme.backgroundColor
    border.color: Kirigami.ColorUtils.linearInterpolation(Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, 0.15)
    Kirigami.Theme.colorSet: Kirigami.Theme.View
    Kirigami.Theme.inherit: false

    ColumnLayout {
        id: column

        anchors.fill: parent
        anchors.margins: Look.largeSpacing
        spacing: Look.largeSpacing
    }
}
