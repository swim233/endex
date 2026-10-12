import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// A framed group of rows.
Rectangle {
    default property alias content: column.data
    property alias spacing: column.spacing

    Layout.fillWidth: true
    implicitWidth: column.implicitWidth + Kirigami.Units.largeSpacing * 2
    implicitHeight: column.implicitHeight + Kirigami.Units.largeSpacing * 2
    radius: Kirigami.Units.cornerRadius
    color: Kirigami.Theme.backgroundColor
    border.color: Kirigami.ColorUtils.linearInterpolation(Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, 0.15)
    Kirigami.Theme.colorSet: Kirigami.Theme.View
    Kirigami.Theme.inherit: false

    ColumnLayout {
        id: column

        anchors.fill: parent
        anchors.margins: Kirigami.Units.largeSpacing
        spacing: Kirigami.Units.largeSpacing
    }
}
