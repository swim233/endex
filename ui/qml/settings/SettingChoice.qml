pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import Endex

// Segmented buttons for one settings key; `options` are [value, label] pairs.
Row {
    id: choice

    required property Settings settings
    required property string key
    required property var options

    Repeater {
        model: choice.options

        QQC2.Button {
            id: option

            required property var modelData

            objectName: choice.key + "." + modelData[0]
            text: modelData[1]
            checkable: true
            checked: choice.settings.draft[choice.key] === modelData[0]
            onClicked: {
                choice.settings.set(choice.key, option.modelData[0])
                checked = Qt.binding(() => choice.settings.draft[choice.key] === option.modelData[0])
            }
        }
    }
}
