import QtQuick
import QtQuick.Controls as QQC2
import Endex

// A drop-down for one settings key; `options` are [value, label] pairs.
QQC2.ComboBox {
    id: combo

    required property Settings settings
    required property string key
    required property var options

    objectName: key
    model: options.map(o => o[1])
    currentIndex: options.findIndex(o => o[0] === settings.draft[key])
    onActivated: index => {
        settings.set(key, options[index][0])
        currentIndex = Qt.binding(() => options.findIndex(o => o[0] === settings.draft[key]))
    }
}
