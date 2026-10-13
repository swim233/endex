import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

// Explanatory small print under a setting.
QQC2.Label {
    Layout.fillWidth: true
    // Readable lines, and forms that keep their labels beside the fields.
    Layout.maximumWidth: Kirigami.Units.gridUnit * 30
    textFormat: Text.PlainText
    wrapMode: Text.WrapAtWordBoundaryOrAnywhere
    font: Look.smallFont
    color: Kirigami.Theme.disabledTextColor
}
