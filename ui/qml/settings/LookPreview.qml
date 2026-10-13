pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

// A small stand-in for the popup in the draft's colours, font and shape (Appearance.preview),
// over a busy backdrop so that translucency shows.
Item {
    id: preview

    // Appearance.preview(): colours, family, fontSize, radius, density, opacity.
    required property var look

    readonly property real unit: Math.round(Kirigami.Units.largeSpacing * (look.density ?? 1))

    implicitWidth: Kirigami.Units.gridUnit * 22
    implicitHeight: Kirigami.Units.gridUnit * 11

    Rectangle {
        id: backdrop

        anchors.fill: parent
        radius: Look.radius
        clip: true
        gradient: Gradient {
            orientation: Gradient.Horizontal
            GradientStop { position: 0; color: "#3d6a8c" }
            GradientStop { position: 0.55; color: "#c97b4a" }
            GradientStop { position: 1; color: "#2f7d5b" }
        }

        Repeater {
            model: 7

            Rectangle {
                required property int index

                x: preview.width * (0.08 + index * 0.13)
                y: preview.height * (index % 2 ? 0.15 : 0.55)
                width: preview.width * 0.08
                height: preview.height * 0.35
                radius: 3
                color: index % 3 ? "#f2f2f2" : "#1b1e20"
                opacity: 0.8
            }
        }
    }

    Rectangle {
        id: panel

        anchors.fill: parent
        anchors.margins: Kirigami.Units.gridUnit
        radius: (preview.look.radius ?? 5) * 2
        color: Qt.alpha(preview.look.window ?? "white", preview.look.opacity ?? 1)
        border.color: preview.look.border ?? "gray"

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: preview.unit * 1.5
            spacing: preview.unit

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: query.implicitHeight + preview.unit
                radius: preview.look.radius ?? 5
                color: preview.look.view ?? "white"
                border.color: preview.look.accent ?? "blue"

                Text {
                    id: query

                    anchors.verticalCenter: parent.verticalCenter
                    x: preview.unit
                    text: qsTr("灯塔")
                    color: preview.look.text ?? "black"
                    font.family: preview.look.family ?? ""
                    font.pointSize: (preview.look.fontSize ?? 10) * 1.35
                }
            }

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: row.implicitHeight + preview.unit
                color: Qt.alpha(preview.look.accent ?? "blue", 0.2)

                Rectangle {
                    width: 3
                    height: parent.height
                    color: preview.look.accent ?? "blue"
                }
                RowLayout {
                    id: row

                    anchors.verticalCenter: parent.verticalCenter
                    x: preview.unit * 1.5
                    width: parent.width - preview.unit * 2.5
                    spacing: preview.unit

                    Text {
                        Layout.fillWidth: true
                        text: qsTr("灯塔守望者笔记.md")
                        elide: Text.ElideRight
                        color: preview.look.text ?? "black"
                        font.family: preview.look.family ?? ""
                        font.pointSize: preview.look.fontSize ?? 10
                        font.bold: true
                    }
                    Text {
                        text: "0.82"
                        color: preview.look.accentText ?? "blue"
                        font.family: "monospace"
                        font.pointSize: (preview.look.fontSize ?? 10) * 0.85
                    }
                    Rectangle {
                        implicitWidth: pill.implicitWidth + preview.unit
                        implicitHeight: pill.implicitHeight
                        radius: height / 2
                        color: Qt.alpha(preview.look.positive ?? "green", 0.15)

                        Text {
                            id: pill

                            anchors.centerIn: parent
                            text: qsTr("关键词 #2")
                            color: preview.look.positive ?? "green"
                            font.family: preview.look.family ?? ""
                            font.pointSize: (preview.look.fontSize ?? 10) * 0.8
                        }
                    }
                }
            }

            Text {
                Layout.fillWidth: true
                Layout.leftMargin: preview.unit * 1.5
                text: qsTr("老灯塔每晚日落后点灯，守望者先擦亮透镜。")
                elide: Text.ElideRight
                color: preview.look.muted ?? "gray"
                font.family: preview.look.family ?? ""
                font.pointSize: preview.look.fontSize ?? 10
            }

            Item {
                Layout.fillHeight: true
            }

            RowLayout {
                spacing: preview.unit

                Rectangle {
                    implicitWidth: Kirigami.Units.smallSpacing * 2
                    implicitHeight: implicitWidth
                    radius: width / 2
                    color: preview.look.positive ?? "green"
                }
                Text {
                    text: qsTr("索引就绪 · 3 个文件")
                    color: preview.look.muted ?? "gray"
                    font.family: preview.look.family ?? ""
                    font.pointSize: (preview.look.fontSize ?? 10) * 0.85
                }
            }
        }
    }
}
