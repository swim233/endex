import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami

// Relevance of one result: cosine in vector mode, BM25 in keyword mode; in hybrid mode the
// cosine plus the keyword rank when the keyword side matched too.
Row {
    id: relevance

    property string mode
    property double vecScore: -1
    property double kwScore: -1
    property int kwRank: 0

    spacing: Kirigami.Units.smallSpacing

    QQC2.Label {
        textFormat: Text.PlainText
        readonly property double value: relevance.mode === "keyword" ? relevance.kwScore : relevance.vecScore

        visible: value >= 0
        anchors.verticalCenter: parent.verticalCenter
        text: value.toFixed(2)
        font.family: "monospace"
        font.pointSize: Kirigami.Theme.smallFont.pointSize
        color: Kirigami.Theme.linkColor
    }

    Rectangle {
        visible: relevance.mode === "hybrid" && relevance.kwRank > 0
        anchors.verticalCenter: parent.verticalCenter
        width: tag.implicitWidth + Kirigami.Units.smallSpacing * 2
        height: tag.implicitHeight
        radius: height / 2
        color: Qt.alpha(Kirigami.Theme.positiveTextColor, 0.15)

        QQC2.Label {
            id: tag

            textFormat: Text.PlainText
            anchors.centerIn: parent
            text: qsTr("关键词 #%1").arg(relevance.kwRank)
            font.pointSize: Kirigami.Theme.smallFont.pointSize * 0.9
            color: Kirigami.Theme.positiveTextColor
        }
    }
}
