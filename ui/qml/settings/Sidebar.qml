pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

// The settings window's categories, a filter, and where the settings live.
ColumnLayout {
    id: sidebar

    // [{id, label, icon}]
    required property var pages
    required property string page
    // Ids of pages with unapplied edits, and with errors from the last apply.
    required property var edited
    required property var failed
    required property string configPath
    // A read-only daemon applies nothing.
    required property bool writable

    signal pageRequested(string id)

    spacing: 0

    Kirigami.SearchField {
        id: filter

        Layout.fillWidth: true
        Layout.margins: Kirigami.Units.smallSpacing
        placeholderText: qsTr("搜索设置…")
    }

    ListView {
        Layout.fillWidth: true
        Layout.fillHeight: true
        clip: true
        model: sidebar.pages.filter(p => p.label.includes(filter.text.trim()))

        delegate: QQC2.ItemDelegate {
            id: category

            required property var modelData
            readonly property bool failed: sidebar.failed.includes(modelData.id)

            objectName: "page." + modelData.id
            width: ListView.view.width
            highlighted: sidebar.page === modelData.id
            onClicked: sidebar.pageRequested(modelData.id)

            contentItem: RowLayout {
                spacing: Kirigami.Units.largeSpacing

                Kirigami.Icon {
                    source: category.modelData.icon
                    implicitWidth: Kirigami.Units.iconSizes.smallMedium
                    implicitHeight: implicitWidth
                }
                QQC2.Label {
                    Layout.fillWidth: true
                    textFormat: Text.PlainText
                    text: category.modelData.label
                    elide: Text.ElideRight
                }
                Kirigami.Icon {
                    visible: category.failed || sidebar.edited.includes(category.modelData.id)
                    source: category.failed ? "dialog-error" : "document-edit"
                    implicitWidth: Kirigami.Units.iconSizes.small
                    implicitHeight: implicitWidth
                }
            }
        }

        Hint {
            anchors.centerIn: parent
            width: parent.width - Kirigami.Units.largeSpacing * 2
            horizontalAlignment: Text.AlignHCenter
            visible: parent.count === 0
            text: qsTr("没有匹配的类别")
        }
    }

    ColumnLayout {
        Layout.fillWidth: true
        Layout.margins: Kirigami.Units.largeSpacing
        spacing: Kirigami.Units.smallSpacing

        Hint {
            text: qsTr("配置文件（唯一来源）")
        }
        Kirigami.SelectableLabel {
            Layout.fillWidth: true
            textFormat: Text.PlainText
            text: sidebar.configPath
            font.family: "monospace"
            font.pointSize: Look.smallFont.pointSize
            wrapMode: Text.WrapAnywhere
        }
        Hint {
            visible: sidebar.writable
            text: qsTr("点「应用」后写入，守护进程自动重载。")
        }
    }
}
