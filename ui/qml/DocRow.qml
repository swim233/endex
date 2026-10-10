import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

Rectangle {
    id: row

    required property int index
    required property string title
    required property string name
    required property string dir
    required property string location
    required property string snippet
    required property var spans
    required property string when
    required property string icon
    required property double vecScore
    required property double kwScore
    required property int kwRank
    property bool selected
    property string mode
    property Results results

    signal clicked()
    signal doubleClicked()
    signal rightClicked()

    implicitHeight: layout.implicitHeight + Kirigami.Units.largeSpacing * 2
    color: selected ? Qt.alpha(Kirigami.Theme.highlightColor, 0.2)
        : hover.hovered ? Qt.alpha(Kirigami.Theme.highlightColor, 0.07) : "transparent"

    Rectangle {
        visible: row.selected
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        width: 3
        color: Kirigami.Theme.highlightColor
    }

    HoverHandler {
        id: hover
    }

    TapHandler {
        onTapped: row.clicked()
        onDoubleTapped: row.doubleClicked()
    }

    TapHandler {
        acceptedButtons: Qt.RightButton
        onTapped: row.rightClicked()
    }

    GridLayout {
        id: layout

        anchors.fill: parent
        anchors.margins: Kirigami.Units.largeSpacing
        anchors.leftMargin: Kirigami.Units.largeSpacing * 1.5
        columns: 2
        columnSpacing: Kirigami.Units.largeSpacing
        rowSpacing: 2

        Kirigami.Icon {
            Layout.rowSpan: 3
            Layout.alignment: Qt.AlignTop
            implicitWidth: Kirigami.Units.iconSizes.medium
            implicitHeight: implicitWidth
            source: row.icon
            fallback: "text-x-generic"
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.largeSpacing

            QQC2.Label {
                textFormat: Text.PlainText
                Layout.fillWidth: true
                text: row.title || row.name
                font.bold: true
                elide: Text.ElideRight
            }
            Relevance {
                mode: row.mode
                vecScore: row.vecScore
                kwScore: row.kwScore
                kwRank: row.kwRank
            }
            QQC2.Label {
                textFormat: Text.PlainText
                text: row.when
                font: Kirigami.Theme.smallFont
                color: Kirigami.Theme.disabledTextColor
            }
        }

        QQC2.Label {
            Layout.fillWidth: true
            Layout.maximumHeight: fontMetrics.lineSpacing * 2
            text: row.results.markup(row.snippet, row.spans, Kirigami.ColorUtils.tintWithAlpha(Kirigami.Theme.backgroundColor, Kirigami.Theme.neutralTextColor, 0.35), Kirigami.Theme.textColor)
            textFormat: Text.RichText
            wrapMode: Text.Wrap
            clip: true

            FontMetrics {
                id: fontMetrics
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.largeSpacing

            QQC2.Label {
                textFormat: Text.PlainText
                Layout.maximumWidth: row.width * 0.5
                text: row.dir + "/" + row.name
                font: Kirigami.Theme.smallFont
                color: Kirigami.Theme.disabledTextColor
                elide: Text.ElideMiddle
            }
            QQC2.Label {
                textFormat: Text.PlainText
                Layout.fillWidth: true
                text: row.location ? "§ " + row.location : ""
                font: Kirigami.Theme.smallFont
                color: Kirigami.Theme.disabledTextColor
                elide: Text.ElideRight
            }
        }
    }
}
