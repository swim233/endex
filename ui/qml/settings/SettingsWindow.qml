pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Dialogs
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

// The settings window: nine pages, edits applied to config.toml (via endexd) or reverted as
// a whole.
Kirigami.ApplicationWindow {
    id: window

    required property Popup popup
    required property Daemon daemon
    required property Settings settings
    required property EvalRun evalRun

    property string page: "dirs"
    // First run: the folders page greets.
    property bool welcome
    // The time of the last successful apply.
    property string appliedAt
    // Close once the pending apply succeeds (the close dialog's 「应用」).
    property bool closeAfterApply
    // A call that failed, until dismissed.
    property string failure

    readonly property var pages: [
        { id: "dirs", label: qsTr("索引目录"), icon: "folder", sub: qsTr("配置要建立索引的文件夹") },
        { id: "exclude", label: qsTr("排除规则"), icon: "security-high", sub: qsTr("选择不需要被索引的目录") },
        { id: "watch", label: qsTr("增量与监听"), icon: "view-refresh", sub: qsTr("控制增量与监听行为") },
        { id: "model", label: qsTr("模型与设备"), icon: "computer", sub: qsTr("嵌入模型、各场景使用的设备，以及截图 OCR。") },
        { id: "status", label: qsTr("索引状态"), icon: "server-database", sub: qsTr("索引的进度、数量，以及处理失败的文件。") },
        { id: "shortcut", label: qsTr("快捷键指引"), icon: "input-keyboard", sub: qsTr("更改 Endex 的快捷键行为") },
        { id: "resource", label: qsTr("性能"), icon: "battery-good", sub: qsTr("控制索引占用电量、处理器和显存。") },
        { id: "eval", label: qsTr("评测与日志"), icon: "office-chart-line", sub: qsTr("用固定查询集衡量检索质量，并决定是否记录使用日志。") },
        { id: "appearance", label: qsTr("外观"), icon: "preferences-desktop-color", sub: qsTr("改 Endex 的样子：颜色、字体、间距、圆角、浮窗大小和半透明。预览跟着草稿走，点「应用」才写入 config.toml。设置窗口里的控件外形仍由 Plasma 决定。") },
    ]
    readonly property var current: pages.find(p => p.id === page) ?? pages[0]
    readonly property var status: daemon.status
    readonly property bool reachable: [Daemon.Ready, Daemon.Empty, Daemon.Broken, Daemon.Rebuilding].includes(daemon.state)
    readonly property bool writable: reachable && status.mode === "writer"
    // endexd came up after the window opened (「启动」, systemd, a restart): read the settings
    // again; unapplied edits are kept.
    onReachableChanged: {
        if (reachable && visible) {
            failure = ""
            settings.load()
        }
    }
    readonly property var errorKeys: Object.keys(settings.errors)

    // The page that shows a settings key.
    function pageOf(key) {
        if (key.startsWith("index.sources") || key.startsWith("index.declined"))
            return "dirs"
        if (key.startsWith("index.") || key.startsWith("exclude."))
            return "exclude"
        if (key.startsWith("update."))
            return "watch"
        if (key.startsWith("embedding.") || key.startsWith("ocr.") || key.startsWith("devices."))
            return "model"
        if (key.startsWith("resources."))
            return "resource"
        if (key.startsWith("log."))
            return "eval"
        if (key.startsWith("appearance."))
            return "appearance"
        return ""
    }

    // From C++ (ShowSettings, the popup): an empty id keeps the page.
    function showPage(id, greet) {
        failure = ""
        if (pages.some(p => p.id === id))
            page = id
        if (greet) {
            welcome = true
            page = "dirs"
        }
        settings.load()
        daemon.refresh()
    }

    // A folder from the folder dialog (or the test hook).
    function addFolder(url) {
        const path = settings.configPath(url)
        const sources = settings.draft["index.sources"] ?? []
        if (!sources.some(s => s.path === path))
            settings.set("index.sources", sources.concat([{ path: path, kind: "all" }]))
        const declined = settings.draft["index.declined"] ?? []
        if (declined.includes(path))
            settings.set("index.declined", declined.filter(p => p !== path))
        page = "dirs"
        return path
    }

    function copy(text) {
        popup.copy(text)
        showPassiveNotification(qsTr("已复制：%1").arg(text))
    }

    function debugState() {
        const sources = (settings.draft["index.sources"] ?? []).map(s => `${s.path}:${s.kind}`).join(",")
        return `visible=${visible} active=${active} page=${page} loaded=${settings.loaded} welcome=${welcome} pending=${settings.pending.length}`
            + ` keys=${settings.pending.join(",")} errors=${errorKeys.join(",")} applying=${settings.applying}`
            + ` sources=${sources} declined=${(settings.draft["index.declined"] ?? []).join(",")}`
            + ` closeDialog=${closeDialog.visible} appliedAt=${appliedAt} failure=${failure}`
            + ` index=${daemon.indexState.state ?? "-"} reason=${daemon.indexState.reason ?? ""}`
            + ` rebuilding=${daemon.indexState.rebuilding ?? false} rebuildPending=${daemon.indexState.rebuild_pending ?? false}`
            + ` configError=${!!status.config_error} restart=${!!status.restart_required}`
            + ` restarting=${daemon.restarting} restartFailed=${!!daemon.restartError}`
            + ` unavailable=${(status.sources ?? []).filter(s => !s.exists).length} usageLog=${settings.usageLog.length}`
            + ` eval=${evalRun.running ? "running" : evalRun.error ? "error" : (evalRun.results.sets ?? []).length + " sets"}`
            + ` bg=${Kirigami.Theme.backgroundColor} text=${Kirigami.Theme.textColor} font=${Kirigami.Theme.defaultFont.family}/${Kirigami.Theme.defaultFont.pointSize}`
            + ` small=${Look.smallFont.family}/${Look.smallFont.pointSize} blur=${Appearance.blurAvailable}`
            + ` theme=${Appearance.themeKeys.join(",")} themeError=${Appearance.themeError}`
            + (pageLoader.item?.debugState ? " " + pageLoader.item.debugState() : "")
    }

    title: qsTr("Endex 设置")
    width: Kirigami.Units.gridUnit * 58
    height: Kirigami.Units.gridUnit * 40
    minimumWidth: Kirigami.Units.gridUnit * 42
    minimumHeight: Kirigami.Units.gridUnit * 26
    pageStack.globalToolBar.style: Kirigami.ApplicationHeaderStyle.None
    pageStack.initialPage: content

    onClosing: close => {
        if (settings.pending.length > 0 && writable) {
            close.accepted = false
            closeDialog.open()
        }
    }

    Connections {
        target: window.settings

        function onApplied(needsRebuild, restartRequired) {
            window.appliedAt = new Date().toLocaleTimeString(Qt.locale(), "HH:mm:ss")
            if ((window.settings.saved["index.sources"] ?? []).length > 0)
                window.welcome = false
            window.daemon.refresh()
            if (window.closeAfterApply) {
                window.closeAfterApply = false
                window.close()
            }
        }

        function onFailed(message) {
            window.closeAfterApply = false
            window.failure = message
        }

        function onErrorsChanged() {
            window.closeAfterApply = false
        }
    }

    Connections {
        target: window.daemon

        function onActionFailed(message) {
            window.failure = message
        }
    }

    // Counts, folders and power change while the window is open.
    Timer {
        interval: 2000
        repeat: true
        running: window.visible
        onTriggered: window.daemon.refresh()
    }

    FolderDialog {
        id: folderDialog

        title: qsTr("选择要索引的文件夹")
        onAccepted: window.addFolder(selectedFolder)
    }

    Kirigami.PromptDialog {
        id: closeDialog

        title: qsTr("还有改动没有应用")
        subtitle: qsTr("有 %1 处改动还没有写入 config.toml。").arg(window.settings.pending.length)
        standardButtons: Kirigami.Dialog.NoButton
        customFooterActions: [
            Kirigami.Action {
                objectName: "closeApply"
                text: qsTr("应用")
                icon.name: "dialog-ok-apply"
                onTriggered: {
                    window.closeAfterApply = true
                    window.settings.apply()
                    closeDialog.close()
                }
            },
            Kirigami.Action {
                objectName: "closeDiscard"
                text: qsTr("放弃改动")
                icon.name: "edit-undo"
                onTriggered: {
                    window.settings.revert()
                    closeDialog.close()
                    window.close()
                }
            },
            Kirigami.Action {
                objectName: "closeCancel"
                text: qsTr("取消")
                icon.name: "dialog-cancel"
                onTriggered: closeDialog.close()
            },
        ]
    }

    Kirigami.Page {
        id: content

        padding: 0
        globalToolBarStyle: Kirigami.ApplicationHeaderStyle.None

        RowLayout {
            anchors.fill: parent
            spacing: 0

            Sidebar {
                Layout.preferredWidth: Kirigami.Units.gridUnit * 13
                Layout.fillHeight: true
                pages: window.pages
                page: window.page
                edited: window.settings.pending.map(k => window.pageOf(k))
                failed: window.errorKeys.map(k => window.pageOf(k))
                configPath: window.settings.saved["config_path"] ?? window.status.config_path ?? ""
                writable: window.writable
                onPageRequested: id => window.page = id
            }

            Kirigami.Separator {
                Layout.fillHeight: true
            }

            ColumnLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 0

                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.margins: Look.largeSpacing * 2
                    Layout.bottomMargin: Look.largeSpacing
                    spacing: Kirigami.Units.smallSpacing

                    Kirigami.Heading {
                        textFormat: Text.PlainText
                        level: 1
                        text: window.current.label
                    }
                    Hint {
                        text: window.current.sub
                    }
                }

                Notices {
                    Layout.fillWidth: true
                    Layout.leftMargin: Look.largeSpacing * 2
                    Layout.rightMargin: Look.largeSpacing * 2
                    daemon: window.daemon
                    settings: window.settings
                    page: window.page
                    failure: window.failure
                    onPageRequested: id => window.page = id
                    onCopy: text => window.copy(text)
                    onOpenFile: path => window.popup.open(path)
                    onDismissed: window.failure = ""
                }

                QQC2.ScrollView {
                    id: scroll

                    objectName: "pageScroll"
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    contentWidth: availableWidth

                    Loader {
                        id: pageLoader

                        x: Look.largeSpacing * 2
                        y: Look.largeSpacing
                        width: scroll.availableWidth - Look.largeSpacing * 4
                        height: item ? item.implicitHeight + Look.largeSpacing * 2 : 0
                        // Pages that edit settings wait for them, and are read-only when the
                        // daemon is.
                        enabled: window.writable || ["status", "shortcut", "eval"].includes(window.page)
                        sourceComponent: {
                            if (!window.settings.loaded && window.page !== "shortcut")
                                return loadingPage
                            switch (window.page) {
                            case "exclude":
                                return excludePage
                            case "watch":
                                return watchPage
                            case "model":
                                return modelPage
                            case "status":
                                return statusPage
                            case "shortcut":
                                return shortcutPage
                            case "resource":
                                return resourcePage
                            case "eval":
                                return evalPage
                            case "appearance":
                                return appearancePage
                            default:
                                return dirsPage
                            }
                        }
                    }
                }

                Kirigami.Separator {
                    Layout.fillWidth: true
                }

                RowLayout {
                    Layout.fillWidth: true
                    Layout.margins: Kirigami.Units.largeSpacing
                    spacing: Kirigami.Units.largeSpacing

                    readonly property int pending: window.settings.pending.length
                    readonly property int problems: window.errorKeys.length

                    Kirigami.Icon {
                        implicitWidth: Kirigami.Units.iconSizes.small
                        implicitHeight: implicitWidth
                        source: parent.problems > 0 ? "dialog-error" : parent.pending > 0 ? "dialog-warning" : "dialog-ok"
                    }
                    QQC2.Label {
                        Layout.fillWidth: true
                        textFormat: Text.PlainText
                        elide: Text.ElideRight
                        color: parent.problems > 0 ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor
                        text: {
                            const footer = parent
                            if (window.settings.applying)
                                return qsTr("正在应用…")
                            if (footer.problems > 0)
                                return qsTr("有 %1 处设置有误，没有写入 config.toml").arg(footer.problems)
                            if (footer.pending > 0)
                                return qsTr("有 %1 处改动还没应用，应用后才写入 config.toml").arg(footer.pending)
                            if (window.appliedAt)
                                return qsTr("已应用并写入 config.toml，守护进程已重载 · %1").arg(window.appliedAt)
                            return qsTr("没有未应用的改动")
                        }
                    }
                    QQC2.Button {
                        objectName: "revertButton"
                        text: qsTr("还原")
                        icon.name: "edit-undo"
                        enabled: parent.pending > 0 && !window.settings.applying
                        onClicked: window.settings.revert()
                    }
                    QQC2.Button {
                        objectName: "applyButton"
                        text: qsTr("应用")
                        icon.name: "dialog-ok-apply"
                        highlighted: enabled
                        enabled: parent.pending > 0 && window.writable && !window.settings.applying
                        onClicked: window.settings.apply()
                    }
                }
            }
        }
    }

    Component {
        id: loadingPage

        ColumnLayout {
            spacing: Kirigami.Units.largeSpacing

            QQC2.BusyIndicator {
                Layout.alignment: Qt.AlignHCenter
                Layout.topMargin: Kirigami.Units.gridUnit * 4
                visible: window.reachable || window.daemon.state === Daemon.Connecting
                running: visible
            }
            QQC2.Label {
                objectName: "loading"
                Layout.alignment: Qt.AlignHCenter
                textFormat: Text.PlainText
                color: Kirigami.Theme.disabledTextColor
                text: window.reachable || window.daemon.state === Daemon.Connecting
                    ? qsTr("正在读取设置…") : qsTr("守护进程没有运行，读不到设置。")
            }
        }
    }
    Component {
        id: dirsPage

        PageDirs {
            daemon: window.daemon
            settings: window.settings
            welcome: window.welcome
            writable: window.writable
            onAddRequested: folderDialog.open()
        }
    }
    Component {
        id: excludePage

        PageExclude {
            settings: window.settings
        }
    }
    Component {
        id: watchPage

        PageWatch {
            daemon: window.daemon
            settings: window.settings
        }
    }
    Component {
        id: modelPage

        PageModel {
            daemon: window.daemon
            settings: window.settings
        }
    }
    Component {
        id: statusPage

        PageStatus {
            daemon: window.daemon
            settings: window.settings
            onPageRequested: id => window.page = id
        }
    }
    Component {
        id: shortcutPage

        PageShortcut {
            popup: window.popup
            onCopy: text => window.copy(text)
        }
    }
    Component {
        id: resourcePage

        PageResource {
            daemon: window.daemon
            settings: window.settings
        }
    }
    Component {
        id: appearancePage

        PageAppearance {
            settings: window.settings
            onCopy: text => {
                window.popup.copy(text)
                window.showPassiveNotification(qsTr("已复制 theme.toml 示例"))
            }
        }
    }
    Component {
        id: evalPage

        PageEval {
            daemon: window.daemon
            settings: window.settings
            evalRun: window.evalRun
            writable: window.writable
        }
    }
}
