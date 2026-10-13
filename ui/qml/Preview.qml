pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

// The selected result: the chunk with highlights (documents) or the image with its OCR text,
// and the actions.
ColumnLayout {
    id: pane

    // Results.current: what the list shows for the selected file.
    required property var current
    // Daemon.preview: the full chunk, fetched for the selection (or one of its neighbours).
    required property var preview
    required property Results results

    signal act(string action)
    signal chunk(string id)

    readonly property bool hasHit: current.path !== undefined
    readonly property bool isImage: current.kind === "image"
    readonly property bool loaded: hasHit && preview.path === current.path

    spacing: 0

    QQC2.Label {
        textFormat: Text.PlainText
        Layout.fillWidth: true
        Layout.fillHeight: true
        visible: !pane.hasHit
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        text: qsTr("选中一条结果，在这里预览")
        color: Kirigami.Theme.disabledTextColor
    }

    QQC2.ScrollView {
        id: scroll

        Layout.fillWidth: true
        Layout.fillHeight: true
        visible: pane.hasHit
        QQC2.ScrollBar.horizontal.policy: QQC2.ScrollBar.AlwaysOff

        ColumnLayout {
            width: scroll.availableWidth
            spacing: Look.largeSpacing

            Rectangle {
                Layout.fillWidth: true
                Layout.topMargin: Look.largeSpacing
                Layout.leftMargin: Look.largeSpacing * 1.5
                Layout.rightMargin: Look.largeSpacing * 1.5
                Layout.preferredHeight: Kirigami.Units.gridUnit * 11
                visible: pane.isImage
                radius: Look.radius
                color: Kirigami.Theme.alternateBackgroundColor

                Image {
                    anchors.fill: parent
                    anchors.margins: 1
                    source: pane.isImage ? pane.current.url : ""
                    asynchronous: true
                    sourceSize: Qt.size(width * Screen.devicePixelRatio, height * Screen.devicePixelRatio)
                    fillMode: Image.PreserveAspectFit
                }
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: pane.isImage ? 0 : Look.largeSpacing * 1.5
                Layout.leftMargin: Look.largeSpacing * 1.5
                Layout.rightMargin: Look.largeSpacing * 1.5
                spacing: Look.largeSpacing

                Kirigami.Icon {
                    visible: !pane.isImage
                    implicitWidth: Kirigami.Units.iconSizes.medium
                    implicitHeight: implicitWidth
                    source: pane.current.icon ?? ""
                    fallback: "text-x-generic"
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0

                    QQC2.Label {
                        textFormat: Text.PlainText
                        Layout.fillWidth: true
                        text: pane.isImage ? pane.current.name ?? "" : pane.current.title || pane.current.name || ""
                        font.bold: true
                        font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.15
                        wrapMode: Text.Wrap
                    }
                    QQC2.Label {
                        textFormat: Text.PlainText
                        Layout.fillWidth: true
                        text: (pane.current.dir ?? "") + "/" + (pane.current.name ?? "")
                        font.family: "monospace"
                        font.pointSize: Look.smallFont.pointSize
                        color: Kirigami.Theme.disabledTextColor
                        elide: Text.ElideMiddle
                    }
                }
            }

            QQC2.Label {
                textFormat: Text.PlainText
                Layout.fillWidth: true
                Layout.leftMargin: Look.largeSpacing * 1.5
                Layout.rightMargin: Look.largeSpacing * 1.5
                text: {
                    const c = pane.current
                    const parts = pane.isImage && c.width ? [`${c.width}×${c.height}`] : []
                    parts.push(c.size ?? "")
                    if (c.when)
                        parts.push(qsTr("修改于 %1").arg(c.when))
                    if (pane.isImage)
                        parts.push(c.ocr ? qsTr("含文字（OCR）") : qsTr("无文字"))
                    return parts.join("  ·  ")
                }
                color: Kirigami.Theme.disabledTextColor
                elide: Text.ElideRight
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.leftMargin: Look.largeSpacing * 1.5
                Layout.rightMargin: Look.largeSpacing
                visible: !pane.isImage
                spacing: Look.smallSpacing

                QQC2.Label {
                    textFormat: Text.PlainText
                    Layout.fillWidth: true
                    text: pane.loaded ? pane.preview.location : pane.current.location ?? ""
                    color: Kirigami.Theme.linkColor
                    elide: Text.ElideRight
                }
                QQC2.ToolButton {
                    visible: pane.loaded && pane.preview.count > 1
                    icon.name: "go-previous"
                    display: QQC2.AbstractButton.IconOnly
                    text: qsTr("上一个片段")
                    enabled: !!pane.preview.prev
                    focusPolicy: Qt.NoFocus
                    onClicked: pane.chunk(pane.preview.prev)
                }
                QQC2.Label {
                    textFormat: Text.PlainText
                    visible: pane.loaded && pane.preview.count > 1
                    text: qsTr("片段 %1 / %2").arg(pane.preview.index + 1).arg(pane.preview.count)
                }
                QQC2.ToolButton {
                    visible: pane.loaded && pane.preview.count > 1
                    icon.name: "go-next"
                    display: QQC2.AbstractButton.IconOnly
                    text: qsTr("下一个片段")
                    enabled: !!pane.preview.next
                    focusPolicy: Qt.NoFocus
                    onClicked: pane.chunk(pane.preview.next)
                }
            }

            QQC2.Label {
                textFormat: Text.PlainText
                Layout.fillWidth: true
                Layout.leftMargin: Look.largeSpacing * 1.5
                Layout.rightMargin: Look.largeSpacing * 1.5
                visible: pane.isImage
                text: pane.current.ocr ? qsTr("OCR 文字（参与关键词检索）") : qsTr("没有识别到文字，只按画面内容（向量）检索")
                font: Look.smallFont
                color: Kirigami.Theme.disabledTextColor
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.leftMargin: Look.largeSpacing * 1.5
                Layout.rightMargin: Look.largeSpacing * 1.5
                Layout.bottomMargin: Look.largeSpacing
                implicitHeight: chunkText.implicitHeight + Look.largeSpacing * 2
                visible: pane.loaded && pane.preview.text !== ""
                radius: Look.radius
                color: Kirigami.Theme.alternateBackgroundColor

                QQC2.Label {
                    id: chunkText

                    anchors.fill: parent
                    anchors.margins: Look.largeSpacing
                    text: pane.loaded
                        ? `<div style="white-space:pre-wrap">${pane.results.markup(pane.preview.text, pane.preview.spans, Kirigami.ColorUtils.tintWithAlpha(Kirigami.Theme.backgroundColor, Kirigami.Theme.neutralTextColor, 0.35), Kirigami.Theme.textColor)}</div>`
                        : ""
                    textFormat: Text.RichText
                    wrapMode: Text.Wrap
                }
            }
        }
    }

    Kirigami.Separator {
        Layout.fillWidth: true
    }

    GridLayout {
        Layout.fillWidth: true
        Layout.margins: Look.largeSpacing
        columns: 6
        rowSpacing: Look.smallSpacing
        columnSpacing: Look.smallSpacing
        enabled: pane.hasHit

        ActionButton {
            Layout.columnSpan: 3
            label: qsTr("打开")
            iconName: "document-open"
            keys: "↵"
            command: "open"
        }
        ActionButton {
            Layout.columnSpan: 3
            label: qsTr("在 Dolphin 中显示")
            iconName: "system-file-manager"
            keys: "Ctrl+↵"
            command: "reveal"
        }
        ActionButton {
            Layout.columnSpan: 2
            label: qsTr("复制路径")
            iconName: "edit-copy-path"
            keys: "Ctrl+C"
            command: "path"
        }
        ActionButton {
            Layout.columnSpan: 2
            label: qsTr("复制片段")
            iconName: "edit-copy"
            keys: "Ctrl+⇧+C"
            command: "snippet"
            enabled: !pane.isImage || !!pane.current.ocr
        }
        ActionButton {
            Layout.columnSpan: 2
            label: qsTr("找相似 · M4")
            iconName: "search"
            keys: "Ctrl+S"
            command: "similar"
            enabled: false
            QQC2.ToolTip.text: qsTr("找相似在 M4 提供")
            QQC2.ToolTip.visible: hovered
        }
    }

    // The desktop style draws `text` and `icon` itself, so the content uses its own properties.
    component ActionButton: QQC2.Button {
        id: button

        property string label
        property string iconName
        property string keys
        property string command

        Accessible.name: label

        Layout.fillWidth: true
        Layout.preferredWidth: 1
        // The desktop style sets no padding; the key cap would sit on the frame.
        padding: Look.smallSpacing
        horizontalPadding: Look.largeSpacing
        focusPolicy: Qt.NoFocus
        onClicked: pane.act(command)

        contentItem: ColumnLayout {
            spacing: 2
            opacity: button.enabled ? 1 : 0.5

            RowLayout {
                spacing: Look.smallSpacing

                Kirigami.Icon {
                    implicitWidth: Kirigami.Units.iconSizes.small
                    implicitHeight: implicitWidth
                    source: button.iconName
                }
                QQC2.Label {
                    textFormat: Text.PlainText
                    Layout.fillWidth: true
                    text: button.label
                    elide: Text.ElideRight
                }
            }
            KeyCap {
                text: button.keys
            }
        }
    }
}
