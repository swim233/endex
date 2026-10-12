import QtQuick
import QtQuick.Controls as QQC2
import Endex

// A number for one settings key.
QQC2.SpinBox {
    id: spin

    required property Settings settings
    required property string key

    objectName: key
    editable: true
    value: settings.draft[key] ?? 0
    onValueModified: {
        settings.set(key, value)
        value = Qt.binding(() => settings.draft[key] ?? 0)
    }
}
