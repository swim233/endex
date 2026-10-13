pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Dialogs
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

// 外观: preset, colours, font, density and radius, the popup's size, translucency and blur, and
// theme.toml. The preview follows the draft; 「应用」 writes config.toml like every page.
ColumnLayout {
    id: page

    required property Settings settings

    signal copy(string text)

    readonly property var draft: settings.draft
    readonly property string preset: draft["appearance.preset"] ?? "plasma"
    // What the controls show: under a preset, its values.
    readonly property var values: Object.assign({}, draft, Appearance.presetValues(preset))
    // The draft's look (Appearance.preview) and theme.toml sample; theme.toml and Plasma's colours
    // change them too.
    property var look: ({})
    property string sample
    readonly property var accents: [["#3daee9", qsTr("蓝")], ["#1d6b57", qsTr("深绿")], ["#27ae60", qsTr("绿")], ["#f67400", qsTr("橙")],
        ["#da4453", qsTr("红")], ["#8e44ad", qsTr("紫")], ["#e91e8c", qsTr("粉")]]
    readonly property var fonts: [
        ["system", qsTr("系统默认"), ""],
        ["noto_sans", qsTr("黑体"), "Noto Sans SC"],
        ["inter", qsTr("拉丁字母更紧凑"), "Inter"],
        ["noto_serif", qsTr("宋体"), "Noto Serif SC"],
        ["jetbrains_mono", qsTr("等宽"), "JetBrains Mono"],
    ]

    function value(key) {
        return values["appearance." + key]
    }

    // A style setting: a preset becomes 自定义, starting from what it showed.
    function setStyle(key, v) {
        if (preset !== "custom")
            choosePreset("custom")
        settings.set("appearance." + key, v)
    }

    function choosePreset(name) {
        const from = Appearance.presetValues(name === "custom" ? preset : name)
        for (const key in from)
            settings.set(key, from[key])
        settings.set("appearance.preset", name)
    }

    function refresh() {
        look = Appearance.preview(draft, page)
        sample = Appearance.sampleToml(draft, page)
    }

    function debugState() {
        return `look=${look.scheme}/${look.accent}/${look.family}/${look.fontSize} warn=${look.accentWarning}`
            + ` missingFont=${look.missingFont} popup=${look.width}x${look.height} clamped=${look.clamped}`
    }

    function presetLabel(id) {
        return { plasma: qsTr("跟随 Plasma"), proto: qsTr("原型样式"), glass: qsTr("毛玻璃"), custom: qsTr("自定义") }[id] ?? id
    }

    function points(pt) {
        return qsTr("%1 pt（≈ %2 px）").arg(Number(pt).toFixed(1)).arg((pt * 96 / 72).toFixed(1))
    }

    spacing: Kirigami.Units.largeSpacing

    onDraftChanged: refresh()
    Component.onCompleted: {
        refresh()
        Appearance.checkBlur()
    }

    // The compositor's blur may be switched on or off in System Settings meanwhile.
    Timer {
        interval: 3000
        repeat: true
        running: page.visible
        onTriggered: Appearance.checkBlur()
    }

    Connections {
        target: Appearance

        function onChanged() {
            page.refresh()
        }

        function onThemeChanged() {
            page.refresh()
        }

        function onEffectSettingsFailed(message) {
            effectError.text = qsTr("没能打开系统设置：%1").arg(message)
        }
    }

    Notice {
        Layout.fillWidth: true
        visible: Appearance.themeKeys.length > 0
        type: Kirigami.MessageType.Warning
        text: qsTr("theme.toml 设定的项优先：%1。下面改了这些项也看不到效果。").arg(Appearance.themeKeys.join("、"))
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: Kirigami.Units.largeSpacing * 2

        LookPreview {
            objectName: "lookPreview"
            look: page.look
        }
        ColumnLayout {
            Layout.alignment: Qt.AlignTop
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            QQC2.Label {
                textFormat: Text.PlainText
                font.bold: true
                text: qsTr("预览")
            }
            Hint {
                text: qsTr("%1 · %2 · %3 pt").arg(page.presetLabel(page.preset)).arg(page.look.scheme === "dark" ? qsTr("深色") : qsTr("浅色"))
                    .arg(Number(page.look.fontSize ?? 10).toFixed(1))
            }
            Hint {
                text: qsTr("颜色和形状跟着草稿走；点「应用」后，浮窗下次唤出时、设置窗口立即换上。")
            }
        }
    }

    Kirigami.FormLayout {
        Layout.fillWidth: true

        Kirigami.Separator {
            Kirigami.FormData.isSection: true
            Kirigami.FormData.label: qsTr("样式")
        }
        ColumnLayout {
            Kirigami.FormData.label: qsTr("预设：")
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            Segmented {
                key: "preset"
                current: page.preset
                options: [["plasma", qsTr("跟随 Plasma")], ["proto", qsTr("原型样式")], ["glass", qsTr("毛玻璃")], ["custom", qsTr("自定义")]]
                onPicked: v => page.choosePreset(v)
            }
            Hint {
                text: {
                    switch (page.preset) {
                    case "proto":
                        return qsTr("最早那版设计稿的样子：绿色强调色、Noto Sans SC、更圆的角。浮窗大小沿用你选的 1100×640。")
                    case "glass":
                        return qsTr("在「跟随 Plasma」的基础上，浮窗半透明，背后的画面做高斯模糊。需要在系统设置里启用 KWin 的「模糊」特效。")
                    case "custom":
                        return qsTr("从当前选择出发，下面每一项都可以单独改。改动任何一项，预设都会变成「自定义」。")
                    default:
                        return qsTr("颜色、字体、控件外形都取自你的 Plasma 主题，换配色方案时 Endex 跟着换。")
                    }
                }
            }
        }
        ColumnLayout {
            Kirigami.FormData.label: qsTr("配色底色：")
            Layout.fillWidth: true
            visible: page.preset === "custom"
            spacing: Kirigami.Units.smallSpacing

            Segmented {
                key: "base"
                current: page.value("base")
                options: [["plasma", qsTr("Plasma（Breeze）")], ["proto", qsTr("原型配色")]]
                onPicked: v => page.setStyle("base", v)
            }
            Hint {
                text: qsTr("自定义时，以哪一套配色为底。")
            }
        }
        ColumnLayout {
            Kirigami.FormData.label: qsTr("明暗：")
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            Segmented {
                key: "scheme"
                current: page.value("scheme")
                options: [["system", qsTr("跟随 Plasma")], ["light", qsTr("浅色")], ["dark", qsTr("深色")]]
                // Not a style key: the preset stays.
                onPicked: v => page.settings.set("appearance.scheme", v)
            }
            Hint {
                text: qsTr("跟随时，随系统配色方案的明暗切换；选浅色或深色，会用 Endex 自带的那一套。")
            }
        }

        Kirigami.Separator {
            Kirigami.FormData.isSection: true
            Kirigami.FormData.label: qsTr("颜色")
        }
        ColumnLayout {
            Kirigami.FormData.label: qsTr("强调色：")
            Kirigami.FormData.labelAlignment: Qt.AlignTop
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            Flow {
                Layout.fillWidth: true
                Layout.maximumWidth: Kirigami.Units.gridUnit * 30
                spacing: Kirigami.Units.smallSpacing

                QQC2.Button {
                    objectName: "appearance.accent.default"
                    text: qsTr("主题默认")
                    checkable: true
                    checked: page.value("accent") === "default"
                    onClicked: {
                        page.setStyle("accent", "default")
                        checked = Qt.binding(() => page.value("accent") === "default")
                    }
                }
                Repeater {
                    model: page.accents

                    QQC2.AbstractButton {
                        id: swatch

                        required property var modelData
                        readonly property bool current: page.value("accent") === modelData[0]

                        objectName: "appearance.accent." + modelData[0]
                        implicitWidth: Kirigami.Units.gridUnit * 1.5
                        implicitHeight: implicitWidth
                        Accessible.name: modelData[1]
                        onClicked: page.setStyle("accent", modelData[0])
                        QQC2.ToolTip.text: modelData[1]
                        QQC2.ToolTip.visible: hovered
                        QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay

                        background: Rectangle {
                            radius: width / 2
                            color: swatch.modelData[0]
                            border.width: swatch.current ? 3 : 1
                            border.color: swatch.current ? Kirigami.Theme.textColor : Qt.alpha(Kirigami.Theme.textColor, 0.25)
                        }
                    }
                }
                RowLayout {
                    spacing: Kirigami.Units.smallSpacing

                    QQC2.Label {
                        textFormat: Text.PlainText
                        text: qsTr("自定义")
                    }
                    QQC2.TextField {
                        id: hex

                        objectName: "appearance.accentHex"
                        implicitWidth: Kirigami.Units.gridUnit * 6
                        placeholderText: "#8e44ad"
                        text: page.value("accent") === "default" ? "" : page.value("accent")
                        validator: RegularExpressionValidator {
                            regularExpression: /#[0-9a-fA-F]{6}/
                        }
                        onAccepted: page.setStyle("accent", text.toLowerCase())
                    }
                    QQC2.Button {
                        text: qsTr("选择…")
                        icon.name: "color-picker"
                        onClicked: colorDialog.open()
                    }
                }
            }
            RowLayout {
                visible: page.look.accentWarning === true
                spacing: Kirigami.Units.smallSpacing

                Kirigami.Icon {
                    implicitWidth: Kirigami.Units.iconSizes.small
                    implicitHeight: implicitWidth
                    source: "dialog-warning"
                }
                Hint {
                    objectName: "accentWarning"
                    color: Kirigami.Theme.neutralTextColor
                    text: qsTr("这个颜色和窗口底色太接近，选中项会看不清。")
                }
            }
            Hint {
                text: qsTr("按钮高亮、选中项、进度条用它。选「主题默认」时，Plasma 预设取系统强调色。")
            }
        }

        Kirigami.Separator {
            Kirigami.FormData.isSection: true
            Kirigami.FormData.label: qsTr("字体")
        }
        ColumnLayout {
            Kirigami.FormData.label: qsTr("字体：")
            Kirigami.FormData.labelAlignment: Qt.AlignTop
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            Flow {
                Layout.fillWidth: true
                Layout.maximumWidth: Kirigami.Units.gridUnit * 30
                spacing: Kirigami.Units.smallSpacing

                Repeater {
                    model: page.fonts

                    QQC2.AbstractButton {
                        id: option

                        required property var modelData
                        readonly property string family: modelData[0] === "system" ? "" : Appearance.installedFamily(modelData[0])
                        readonly property bool missing: modelData[0] !== "system" && family === ""
                        readonly property bool current: page.value("font") === modelData[0]

                        objectName: "appearance.font." + modelData[0]
                        implicitWidth: Kirigami.Units.gridUnit * 9
                        implicitHeight: implicitContentHeight + topPadding + bottomPadding
                        padding: Look.largeSpacing
                        onClicked: page.setStyle("font", modelData[0])

                        background: Rectangle {
                            radius: Look.radius
                            color: option.current ? Qt.alpha(Kirigami.Theme.highlightColor, 0.15) : Kirigami.Theme.backgroundColor
                            border.width: option.current ? 2 : 1
                            border.color: option.current ? Kirigami.Theme.highlightColor : Qt.alpha(Kirigami.Theme.textColor, 0.2)
                            Kirigami.Theme.colorSet: Kirigami.Theme.View
                            Kirigami.Theme.inherit: false
                        }
                        contentItem: ColumnLayout {
                            id: optionColumn

                            spacing: 0
                            opacity: option.missing ? 0.6 : 1

                            QQC2.Label {
                                textFormat: Text.PlainText
                                text: qsTr("灯塔 Aa 123")
                                font.family: option.family || Kirigami.Theme.defaultFont.family
                                font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.2
                            }
                            QQC2.Label {
                                textFormat: Text.PlainText
                                text: option.modelData[1]
                                font: Look.smallFont
                            }
                            QQC2.Label {
                                textFormat: Text.PlainText
                                text: option.missing ? qsTr("%1 · 未安装").arg(option.modelData[2])
                                    : option.modelData[0] === "system" ? Kirigami.Theme.defaultFont.family : option.family
                                font: Look.smallFont
                                color: Kirigami.Theme.disabledTextColor
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }
                        }
                    }
                }
            }
            Hint {
                objectName: "missingFont"
                visible: text !== ""
                color: Kirigami.Theme.neutralTextColor
                text: page.look.missingFont ? qsTr("没有安装 %1，正在用系统字体。").arg(page.look.missingFont) : ""
            }
            Hint {
                text: qsTr("浮窗和设置窗口的字体都随这里变。字体要已安装在系统上，没有安装时用系统字体。")
            }
        }
        ColumnLayout {
            Kirigami.FormData.label: qsTr("字号：")
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            RowLayout {
                QQC2.Slider {
                    objectName: "appearance.font_size"
                    Layout.preferredWidth: Kirigami.Units.gridUnit * 12
                    from: 8
                    to: 14
                    stepSize: 0.5
                    snapMode: QQC2.Slider.SnapAlways
                    value: page.value("font_size") ?? 10
                    onMoved: {
                        page.setStyle("font_size", value)
                        value = Qt.binding(() => page.value("font_size") ?? 10)
                    }
                }
                QQC2.Label {
                    textFormat: Text.PlainText
                    text: page.points(page.value("font_size") ?? 10)
                }
            }
            Hint {
                text: qsTr("10 pt 是 Plasma 常见的默认值。浮窗里的搜索框是它的 1.35 倍。")
            }
        }

        Kirigami.Separator {
            Kirigami.FormData.isSection: true
            Kirigami.FormData.label: qsTr("密度与圆角")
        }
        ColumnLayout {
            Kirigami.FormData.label: qsTr("排版密度：")
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            Segmented {
                key: "density"
                current: page.value("density")
                options: [["compact", qsTr("紧凑")], ["comfortable", qsTr("舒适")]]
                onPicked: v => page.setStyle("density", v)
            }
            Hint {
                text: qsTr("只调 Endex 自己的间距：行距、边距、横幅内边距。")
            }
        }
        ColumnLayout {
            Kirigami.FormData.label: qsTr("圆角：")
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            RowLayout {
                QQC2.Slider {
                    objectName: "appearance.radius"
                    Layout.preferredWidth: Kirigami.Units.gridUnit * 12
                    from: 0
                    to: 16
                    stepSize: 1
                    snapMode: QQC2.Slider.SnapAlways
                    value: page.value("radius") ?? 5
                    onMoved: {
                        page.setStyle("radius", value)
                        value = Qt.binding(() => page.value("radius") ?? 5)
                    }
                }
                QQC2.Label {
                    textFormat: Text.PlainText
                    text: qsTr("%1 px").arg(page.value("radius") ?? 5)
                }
            }
            Hint {
                text: qsTr("浮窗里所有东西都跟着变。设置窗口里只有 Endex 自己画的卡片和提示框会变；按钮、开关的圆角由 Plasma 决定。")
            }
        }

        Kirigami.Separator {
            Kirigami.FormData.isSection: true
            Kirigami.FormData.label: qsTr("搜索浮窗")
        }
        ColumnLayout {
            Kirigami.FormData.label: qsTr("浮窗大小：")
            Kirigami.FormData.labelAlignment: Qt.AlignTop
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            Repeater {
                model: [["default", qsTr("1100 × 640（默认）")], ["small", qsTr("940 × 620（较小）")], ["screen", qsTr("按屏幕比例")], ["custom", qsTr("自定义大小")]]

                QQC2.RadioButton {
                    required property var modelData

                    objectName: "appearance.popup_size." + modelData[0]
                    text: modelData[1]
                    checked: page.value("popup_size") === modelData[0]
                    onClicked: page.setStyle("popup_size", modelData[0])
                }
            }
            RowLayout {
                visible: page.value("popup_size") === "screen"

                QQC2.Label {
                    textFormat: Text.PlainText
                    text: qsTr("屏幕占比：")
                }
                QQC2.Slider {
                    objectName: "appearance.screen_percent"
                    Layout.preferredWidth: Kirigami.Units.gridUnit * 10
                    from: 40
                    to: 90
                    stepSize: 5
                    snapMode: QQC2.Slider.SnapAlways
                    value: page.value("screen_percent") ?? 60
                    onMoved: {
                        page.setStyle("screen_percent", value)
                        value = Qt.binding(() => page.value("screen_percent") ?? 60)
                    }
                }
                QQC2.Label {
                    textFormat: Text.PlainText
                    text: qsTr("%1%").arg(page.value("screen_percent") ?? 60)
                }
            }
            RowLayout {
                visible: page.value("popup_size") === "custom"

                QQC2.Label {
                    textFormat: Text.PlainText
                    text: qsTr("宽")
                }
                PixelSpin {
                    key: "popup_width"
                    from: 480
                    to: 1600
                }
                QQC2.Label {
                    textFormat: Text.PlainText
                    text: qsTr("× 高")
                }
                PixelSpin {
                    key: "popup_height"
                    from: 320
                    to: 1000
                }
                QQC2.Label {
                    textFormat: Text.PlainText
                    text: "px"
                }
            }
            QQC2.Label {
                objectName: "popupSize"
                textFormat: Text.PlainText
                text: qsTr("在这块屏幕上实际大小 %1 × %2").arg(page.look.width ?? 0).arg(page.look.height ?? 0)
            }
            Hint {
                visible: page.look.clamped === true
                color: Kirigami.Theme.neutralTextColor
                text: qsTr("设置的 %1 × %2 在这块屏幕上放不下，已缩小到能完整显示。").arg(page.look.wantedWidth ?? 0).arg(page.look.wantedHeight ?? 0)
            }
        }

        Kirigami.Separator {
            Kirigami.FormData.isSection: true
            Kirigami.FormData.label: qsTr("半透明与模糊")
        }
        ColumnLayout {
            Kirigami.FormData.label: qsTr("半透明：")
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            RowLayout {
                QQC2.Slider {
                    objectName: "appearance.translucency"
                    Layout.preferredWidth: Kirigami.Units.gridUnit * 12
                    from: 0
                    to: 60
                    stepSize: 5
                    snapMode: QQC2.Slider.SnapAlways
                    value: page.value("translucency") ?? 0
                    onMoved: {
                        page.setStyle("translucency", value)
                        value = Qt.binding(() => page.value("translucency") ?? 0)
                    }
                }
                QQC2.Label {
                    textFormat: Text.PlainText
                    text: qsTr("%1%").arg(page.value("translucency") ?? 0)
                }
            }
            Hint {
                text: qsTr("0% 是不透明。数值越大，越能透出浮窗后面的窗口。")
            }
        }
        ColumnLayout {
            Kirigami.FormData.label: qsTr("背景模糊：")
            Kirigami.FormData.labelAlignment: Qt.AlignTop
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            QQC2.Switch {
                objectName: "appearance.blur"
                text: qsTr("高斯模糊浮窗背后的画面")
                checked: page.value("blur") === true
                onToggled: {
                    // Read first: switching to 自定义 re-evaluates `checked`.
                    const on = checked
                    // Without translucency the blur would not show.
                    if (on && (page.value("translucency") ?? 0) === 0)
                        page.setStyle("translucency", 30)
                    page.setStyle("blur", on)
                    checked = Qt.binding(() => page.value("blur") === true)
                }
            }
            Hint {
                text: qsTr("靠 KWin 的「模糊」特效：需要在系统设置 → 窗口管理 → 桌面特效里启用「模糊」，没启用时只有半透明。")
            }
            Hint {
                visible: page.value("blur") === true && (page.value("translucency") ?? 0) === 0
                text: qsTr("半透明为 0% 时，模糊看不出来。打开模糊时会自动调到 30%。")
            }
            Hint {
                objectName: "blurUnavailable"
                visible: !Appearance.blurAvailable
                color: Kirigami.Theme.neutralTextColor
                text: qsTr("KWin 现在没有提供模糊：「模糊」特效没有启用，或者当前的显示方式不支持。浮窗只会半透明。")
            }
            Hint {
                text: qsTr("模糊强度由 KWin 统一设置（「模糊」特效里的「模糊强度」），对所有窗口都一样，Endex 自己改不了。")
            }
            QQC2.Button {
                id: effectsButton

                objectName: "openEffects"
                text: qsTr("打开「桌面特效」设置")
                icon.name: "preferences-desktop-effects"
                onClicked: {
                    effectError.text = ""
                    Appearance.openEffectSettings(effectsButton)
                }
            }
            Hint {
                id: effectError

                visible: text !== ""
                color: Kirigami.Theme.negativeTextColor
            }
        }

        Kirigami.Separator {
            Kirigami.FormData.isSection: true
            Kirigami.FormData.label: qsTr("主题文件")
        }
        ColumnLayout {
            Kirigami.FormData.label: qsTr("theme.toml：")
            Kirigami.FormData.labelAlignment: Qt.AlignTop
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            Hint {
                text: qsTr("把当前选择写成 theme.toml，放到 %1。Endex 监听这个文件，保存后立即重载；写了的项优先于上面的设置。")
                    .arg(page.settings.configPath("file://" + Appearance.themePath))
            }
            Hint {
                objectName: "themeStatus"
                color: Appearance.themeError ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.disabledTextColor
                text: Appearance.themeError ? qsTr("theme.toml 有错误，仍在用上一次能用的主题：%1").arg(Appearance.themeError)
                    : Appearance.themeKeys.length > 0 ? qsTr("已载入 theme.toml，它设定了：%1").arg(Appearance.themeKeys.join("、"))
                    : qsTr("现在没有 theme.toml。")
            }
            QQC2.TextArea {
                objectName: "themeSample"
                Layout.fillWidth: true
                Layout.maximumWidth: Kirigami.Units.gridUnit * 30
                readOnly: true
                textFormat: TextEdit.PlainText
                wrapMode: TextEdit.NoWrap
                font.family: "monospace"
                font.pointSize: Look.smallFont.pointSize
                text: page.sample
            }
            QQC2.Button {
                objectName: "copyTheme"
                text: qsTr("复制")
                icon.name: "edit-copy"
                onClicked: page.copy(page.sample)
            }
        }
    }

    ColorDialog {
        id: colorDialog

        title: qsTr("强调色")
        selectedColor: page.value("accent") === "default" ? page.look.accent : page.value("accent")
        onAccepted: page.setStyle("accent", selectedColor.toString().toLowerCase())
    }

    // Buttons side by side, one checked; `options` are [value, label] pairs.
    component Segmented: Row {
        id: segmented

        required property string key
        required property var options
        property string current

        signal picked(string value)

        Repeater {
            model: segmented.options

            QQC2.Button {
                id: segment

                required property var modelData

                objectName: "appearance." + segmented.key + "." + modelData[0]
                text: modelData[1]
                checkable: true
                checked: segmented.current === modelData[0]
                onClicked: {
                    segmented.picked(modelData[0])
                    checked = Qt.binding(() => segmented.current === segment.modelData[0])
                }
            }
        }
    }

    // A number of pixels for the custom popup size.
    component PixelSpin: QQC2.SpinBox {
        id: spin

        required property string key

        objectName: "appearance." + key
        editable: true
        stepSize: 10
        value: page.value(key) ?? 0
        onValueModified: {
            page.setStyle(key, value)
            value = Qt.binding(() => page.value(spin.key) ?? 0)
        }
    }
}
