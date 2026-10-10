pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

// Documents, then images; each group shows one page and pages on its own.
QQC2.ScrollView {
    id: list

    required property Results results
    required property string mode

    signal activated()
    signal contextMenu()
    signal clicked()

    function ensureCurrentVisible() {
        Qt.callLater(() => {
            const group = results.group === 0 ? docs : results.group === 1 ? images : null
            const item = group?.itemAt(results.row)
            if (!item)
                return
            // The first row brings its group header along.
            const top = results.row < (group === docs ? 1 : 3) ? group.header.y : item.mapToItem(content, 0, 0).y
            const bottom = item.mapToItem(content, 0, item.height).y
            if (top < flick.contentY)
                flick.contentY = top
            else if (bottom > flick.contentY + flick.height)
                flick.contentY = Math.min(bottom - flick.height, top)
        })
    }

    QQC2.ScrollBar.horizontal.policy: QQC2.ScrollBar.AlwaysOff

    Flickable {
        id: flick

        contentWidth: width
        contentHeight: content.height
        boundsBehavior: Flickable.StopAtBounds
        clip: true

        Column {
            id: content

            width: flick.width

            GroupHeader {
                id: docsHeader

                visible: list.results.docCount > 0
                label: qsTr("文档")
                count: list.results.docCount
                page: list.results.docPage
                pages: list.results.docPages
                group: 0
                other: list.results.imageCount > 0 ? qsTr("Tab 跳到图片") : ""
            }

            Repeater {
                id: docs

                readonly property Item header: docsHeader

                model: list.results.docs

                DocRow {
                    width: content.width
                    selected: list.results.group === 0 && list.results.row === index
                    mode: list.mode
                    results: list.results
                    onClicked: {
                        list.results.select(0, index)
                        list.clicked()
                    }
                    onDoubleClicked: list.activated()
                    onRightClicked: {
                        list.results.select(0, index)
                        list.contextMenu()
                    }
                }
            }

            GroupHeader {
                id: imagesHeader

                visible: list.results.imageCount > 0
                label: qsTr("图片")
                count: list.results.imageCount
                page: list.results.imagePage
                pages: list.results.imagePages
                group: 1
                other: list.results.docCount > 0 ? qsTr("Tab 跳到文档") : ""
            }

            Grid {
                x: Kirigami.Units.largeSpacing
                width: content.width - 2 * x
                columns: 3
                spacing: Kirigami.Units.smallSpacing
                bottomPadding: Kirigami.Units.largeSpacing

                Repeater {
                    id: images

                    readonly property Item header: imagesHeader

                    model: list.results.images

                    ImageCell {
                        width: Math.floor((content.width - 2 * Kirigami.Units.largeSpacing - 2 * Kirigami.Units.smallSpacing) / 3)
                        selected: list.results.group === 1 && list.results.row === index
                        mode: list.mode
                        results: list.results
                        onClicked: {
                            list.results.select(1, index)
                            list.clicked()
                        }
                        onDoubleClicked: list.activated()
                        onRightClicked: {
                            list.results.select(1, index)
                            list.contextMenu()
                        }
                    }
                }
            }
        }
    }

    component GroupHeader: RowLayout {
        id: header

        property string label
        property int count
        property int page
        property int pages
        property int group
        property string other

        width: content.width
        height: Kirigami.Units.gridUnit * 2
        spacing: Kirigami.Units.smallSpacing

        QQC2.Label {
            textFormat: Text.PlainText
            Layout.leftMargin: Kirigami.Units.largeSpacing * 1.5
            text: header.label
            font.bold: true
        }
        QQC2.Label {
            textFormat: Text.PlainText
            text: qsTr("%1 条").arg(header.count)
            color: Kirigami.Theme.disabledTextColor
        }
        QQC2.ToolButton {
            visible: header.pages > 1
            icon.name: "go-previous"
            display: QQC2.AbstractButton.IconOnly
            text: qsTr("上一页（PgUp）")
            enabled: header.page > 0
            focusPolicy: Qt.NoFocus
            onClicked: list.results.turnPage(header.group, -1)
        }
        QQC2.Label {
            textFormat: Text.PlainText
            visible: header.pages > 1
            text: qsTr("第 %1 / %2 页").arg(header.page + 1).arg(header.pages)
        }
        QQC2.ToolButton {
            visible: header.pages > 1
            icon.name: "go-next"
            display: QQC2.AbstractButton.IconOnly
            text: qsTr("下一页（PgDn）")
            enabled: header.page + 1 < header.pages
            focusPolicy: Qt.NoFocus
            onClicked: list.results.turnPage(header.group, 1)
        }
        Item {
            Layout.fillWidth: true
        }
        QQC2.Label {
            textFormat: Text.PlainText
            Layout.rightMargin: Kirigami.Units.largeSpacing * 1.5
            text: header.other
            font: Kirigami.Theme.smallFont
            color: Kirigami.Theme.disabledTextColor
        }
    }
}
