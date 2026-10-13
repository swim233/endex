import QtQuick
import org.kde.kirigami as Kirigami
import Endex

// A Kirigami notice whose corners follow the 「外观」 radius, like Endex's cards; Plasma's own
// controls keep theirs.
Kirigami.InlineMessage {
    Component.onCompleted: (background as Rectangle).radius = Qt.binding(() => Look.radius)
}
