pragma Singleton

import QtQuick
import org.kde.kirigami as Kirigami
import Endex

// Endex's own shapes, as the 「外观」 settings make them: spacing (density), corner radius,
// popup opacity. Colours and fonts reach Kirigami.Theme through the application's colour
// scheme and font (Appearance); Plasma's controls keep their own shapes.
QtObject {
    readonly property int smallSpacing: Math.round(Kirigami.Units.smallSpacing * Appearance.density)
    readonly property int largeSpacing: Math.round(Kirigami.Units.largeSpacing * Appearance.density)
    readonly property int radius: Appearance.radius
    readonly property int panelRadius: radius * 2
    readonly property real opacity: Appearance.opacity
    readonly property font smallFont: Appearance.customFont ? Appearance.smallFont : Kirigami.Theme.smallFont
}
