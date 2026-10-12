import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import Endex

// A switch for one boolean settings key, with a hint below.
ColumnLayout {
    id: field

    required property Settings settings
    required property string key
    property alias text: toggle.text
    property string hint

    spacing: 0

    QQC2.Switch {
        id: toggle

        objectName: field.key
        checked: field.settings.draft[field.key] === true
        onToggled: {
            field.settings.set(field.key, checked)
            checked = Qt.binding(() => field.settings.draft[field.key] === true)
        }
    }
    Hint {
        visible: text !== ""
        text: field.hint
        Layout.leftMargin: toggle.indicator.width + toggle.spacing
    }
}
