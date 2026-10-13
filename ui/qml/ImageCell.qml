import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

Rectangle {
    id: cell

    required property int index
    required property url url
    required property string name
    required property string snippet
    required property var spans
    required property bool ocr
    required property double vecScore
    required property double kwScore
    required property int kwRank
    property bool selected
    property string mode
    property Results results

    signal clicked()
    signal doubleClicked()
    signal rightClicked()

    implicitHeight: column.implicitHeight + Look.smallSpacing * 2
    radius: Look.radius
    color: selected ? Qt.alpha(Kirigami.Theme.highlightColor, 0.2)
        : hover.hovered ? Qt.alpha(Kirigami.Theme.highlightColor, 0.07) : "transparent"
    border.width: selected ? 2 : 0
    border.color: Kirigami.Theme.highlightColor

    HoverHandler {
        id: hover
    }

    TapHandler {
        onTapped: cell.clicked()
        onDoubleTapped: cell.doubleClicked()
    }

    TapHandler {
        acceptedButtons: Qt.RightButton
        onTapped: cell.rightClicked()
    }

    ColumnLayout {
        id: column

        anchors.fill: parent
        anchors.margins: Look.smallSpacing
        spacing: 2

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: Math.round(width * 0.625)
            radius: Look.radius
            color: Kirigami.Theme.alternateBackgroundColor
            clip: true

            Image {
                id: thumbnail

                anchors.fill: parent
                source: cell.url
                // Decoded at thumbnail size on a worker thread, never on the UI thread.
                asynchronous: true
                sourceSize: Qt.size(320, 200)
                fillMode: Image.PreserveAspectCrop
            }

            Kirigami.Icon {
                anchors.centerIn: parent
                width: Kirigami.Units.iconSizes.medium
                height: width
                visible: thumbnail.status !== Image.Ready
                source: thumbnail.status === Image.Error ? "image-missing" : "image-x-generic"
                color: Kirigami.Theme.disabledTextColor
            }

            Rectangle {
                anchors.top: parent.top
                anchors.right: parent.right
                anchors.margins: Look.smallSpacing
                width: relevance.implicitWidth + Look.smallSpacing * 2
                height: relevance.implicitHeight + 2
                visible: relevance.implicitWidth > 0
                radius: Look.radius
                color: Qt.alpha(Kirigami.Theme.backgroundColor, 0.85)

                Relevance {
                    id: relevance

                    anchors.centerIn: parent
                    mode: cell.mode
                    vecScore: cell.vecScore
                    kwScore: cell.kwScore
                    kwRank: cell.kwRank
                }
            }
        }

        QQC2.Label {
            textFormat: Text.PlainText
            Layout.fillWidth: true
            text: cell.name
            font: Look.smallFont
            elide: Text.ElideMiddle
        }

        QQC2.Label {
            Layout.fillWidth: true
            Layout.maximumHeight: implicitHeight
            text: cell.ocr ? cell.results.markup(cell.snippet, cell.spans, Kirigami.ColorUtils.tintWithAlpha(Kirigami.Theme.backgroundColor, Kirigami.Theme.neutralTextColor, 0.35), Kirigami.Theme.textColor)
                           : qsTr("语义匹配")
            textFormat: cell.ocr ? Text.RichText : Text.PlainText
            font: Look.smallFont
            color: cell.ocr ? Kirigami.Theme.textColor : Kirigami.Theme.disabledTextColor
            wrapMode: Text.NoWrap
            clip: true
        }
    }
}
