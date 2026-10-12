pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

Window {
    id: root

    required property Popup popup
    required property Daemon daemon
    required property Results results
    required property Settings settings

    property string mode: "hybrid"
    readonly property var modes: [
        { id: "hybrid", label: qsTr("混合"), tip: qsTr("关键词与向量融合（RRF）") },
        { id: "vector", label: qsTr("向量"), tip: qsTr("按语义找，一句话也能找到图片") },
        { id: "keyword", label: qsTr("关键词"), tip: qsTr("BM25 + jieba 分词，只匹配文字") },
    ]
    // What the body shows: "results", "tips", "none", "failed", "first" (no folders yet),
    // "empty" (read-only, nothing indexed), "down", "broken", "rebuilding" or "connecting".
    readonly property string view: {
        switch (daemon.state) {
        case Daemon.Down:
        case Daemon.Starting:
            return "down"
        case Daemon.Broken:
            return "broken"
        case Daemon.Rebuilding:
            return "rebuilding"
        case Daemon.Connecting:
            if (results.docCount + results.imageCount === 0)
                return "connecting"
        }
        if (daemon.status.mode === "writer" && (daemon.status.sources ?? []).length === 0)
            return "first"
        if (daemon.state === Daemon.Empty && daemon.status.mode !== "writer")
            return "empty"
        if (field.text.trim() === "")
            return "tips"
        if (daemon.searchError !== "")
            return "failed"
        if (results.docCount + results.imageCount > 0)
            return "results"
        return debounce.running || daemon.searching || results.query !== field.text ? "results" : "none"
    }

    function setMode(id) {
        if (mode === id)
            return
        mode = id
        searched = true
        debounce.stop()
        daemon.search(field.text, mode)
    }

    // Esc: an open menu first, then the popup.
    function stepBack() {
        if (menu.visible)
            menu.close()
        else
            popup.hide()
    }

    function toast(message) {
        status.toast(message)
    }

    // The usage log gets at most one entry per summon, and only when the user did something:
    // a result opened, or the popup hidden after a query the user searched (typed, a recent
    // search, another mode). Not for the automatic re-run of the previous query, not for a
    // cleared field, never per keystroke.
    property var session: ({})
    property bool sessionReported
    property bool searched

    function answered() {
        return {
            query: results.query,
            mode: results.mode,
            results: results.docCount + results.imageCount,
            latency: daemon.latency,
        }
    }

    function report(picked) {
        const entry = picked ? answered() : session
        if (sessionReported || !entry.query || daemon.status.usage_log !== true)
            return
        daemon.reportUsage({
            query: entry.query,
            mode: entry.mode,
            results: entry.results,
            latency_ms: entry.latency,
            picked: picked,
        })
        sessionReported = true
    }

    function openSettings(page, welcome) {
        menu.close()
        popup.openSettings(page, welcome)
    }

    function act(action) {
        const hit = results.current
        if (!hit.path)
            return
        switch (action) {
        case "open":
            report(hit.path)
            popup.open(hit.path)
            break
        case "reveal":
            report(hit.path)
            popup.reveal(hit.path)
            break
        case "path":
            popup.copy(hit.path)
            toast(qsTr("已复制路径：%1").arg(hit.path))
            break
        case "snippet": {
            const full = daemon.previewId !== "" && daemon.preview.path === hit.path ? daemon.preview.text : undefined
            const text = full ?? hit.snippet
            if (!text) {
                toast(qsTr("这张图片没有可复制的文字"))
                break
            }
            popup.copy(text)
            toast(qsTr("已复制片段（%1 字）").arg(text.length))
            break
        }
        case "similar":
            toast(qsTr("「找相似」在 M4 提供"))
            break
        }
    }

    function debugState() {
        const c = results.current
        const p = daemon.preview
        return `visible=${visible} active=${active} focus=${activeFocusItem ? activeFocusItem.objectName : "-"}`
            + ` text=${field.text} preedit=${field.preeditText} mode=${mode} view=${view} daemon=${daemon.state}`
            + ` docs=${results.docCount} images=${results.imageCount} group=${results.group} row=${results.row}`
            + ` docPage=${results.docPage + 1}/${results.docPages} imagePage=${results.imagePage + 1}/${results.imagePages}`
            + ` current=${c.name ?? "-"} chunk=${p.index ?? "-"}/${p.count ?? "-"} menu=${menu.visible}`
            + ` arrows=${results.arrowsInGrid ? "grid" : "field"} autosel=${field.autoSelection}`
            + ` error=${daemon.error} searchError=${daemon.searchError}`
            + ` banner=${banner.kind} index=${daemon.indexState.state ?? "-"} recent=${daemon.recent.length}`
            + ` recentIndex=${stateView.recentIndex} status=${status.stateText}`
            + ` session=${session.query ?? "-"} searched=${searched} reported=${sessionReported} usageLog=${daemon.status.usage_log}`
    }

    width: 1100
    height: 640
    color: "transparent"
    title: qsTr("Endex 搜索")
    // A menu left open must not come back with the next summon.
    onVisibleChanged: {
        if (!visible) {
            menu.close()
            report("")
        }
    }

    Connections {
        target: root.popup

        function onShown(query) {
            root.session = {}
            root.sessionReported = false
            root.searched = false
            stateView.recentIndex = -1
            if (query)
                field.text = query
            field.forceActiveFocus()
            field.selectAll()
            field.autoSelection = true
            root.daemon.refresh()
            root.daemon.fetchRecent()
            if (!root.settings.loaded)
                root.settings.load()
            // One search, not one more from the debounce: the index may have changed since.
            debounce.stop()
            if (field.text.trim() !== "")
                root.daemon.search(field.text, root.mode)
        }

        function onFailed(message) {
            root.toast(message)
        }
    }

    Connections {
        target: root.daemon

        function onRecovered() {
            if (field.text.trim() !== "")
                root.daemon.search(field.text, root.mode)
        }
    }

    // While another process rebuilds the index, look again until it can be searched.
    Timer {
        interval: 2000
        repeat: true
        running: root.visible && root.daemon.state === Daemon.Rebuilding
        onTriggered: root.daemon.refresh()
    }

    Connections {
        target: root.results

        function onSelectionChanged() {
            root.daemon.fetchPreview(root.results.current.id ?? "", root.results.query)
            list.ensureCurrentVisible()
        }

        function onResultsChanged() {
            if (root.results.query.trim() === "")
                root.session = {}
            else if (root.searched)
                root.session = root.answered()
        }
    }

    Timer {
        id: debounce
        interval: 120
        onTriggered: root.daemon.search(field.text, root.mode)
    }

    QQC2.Menu {
        id: menu

        popupType: QQC2.Popup.Item
        onClosed: field.forceActiveFocus()

        QQC2.MenuItem {
            text: qsTr("打开")
            icon.name: "document-open"
            onTriggered: root.act("open")
        }
        QQC2.MenuItem {
            text: qsTr("在 Dolphin 中显示")
            icon.name: "system-file-manager"
            onTriggered: root.act("reveal")
        }
        QQC2.MenuItem {
            text: qsTr("复制路径")
            icon.name: "edit-copy-path"
            onTriggered: root.act("path")
        }
        QQC2.MenuItem {
            text: qsTr("复制片段")
            icon.name: "edit-copy"
            onTriggered: root.act("snippet")
        }
        QQC2.MenuItem {
            text: qsTr("找相似（M4）")
            icon.name: "search"
            enabled: false
        }
    }

    Rectangle {
        id: panel

        anchors.fill: parent
        radius: Kirigami.Units.cornerRadius * 2
        color: Kirigami.Theme.backgroundColor
        border.color: Kirigami.ColorUtils.linearInterpolation(Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, 0.25)
        Kirigami.Theme.colorSet: Kirigami.Theme.Window
        Kirigami.Theme.inherit: false

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 1
            spacing: 0

            RowLayout {
                Layout.fillWidth: true
                Layout.margins: Kirigami.Units.largeSpacing * 1.5
                spacing: Kirigami.Units.largeSpacing

                QQC2.TextField {
                    id: field

                    // The selectAll() of a summon: Ctrl+C still copies the path then. A selection
                    // the user makes is a normal copy.
                    property bool autoSelection

                    objectName: "searchField"
                    Layout.fillWidth: true
                    font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.35
                    leftPadding: searchIcon.width + Kirigami.Units.largeSpacing * 2
                    rightPadding: clearButton.width + Kirigami.Units.largeSpacing
                    placeholderText: qsTr("搜索文档和图片，试试「海边的日落」")
                    // Longer queries are cut by the daemon anyway.
                    maximumLength: 512
                    onTextChanged: {
                        debounce.restart()
                        root.results.releaseArrows()
                        stateView.recentIndex = -1
                    }
                    onTextEdited: root.searched = true
                    onSelectedTextChanged: {
                        if (selectedText !== text)
                            autoSelection = false
                    }

                    Keys.onPressed: event => {
                        const ctrl = event.modifiers & Qt.ControlModifier
                        const shift = event.modifiers & Qt.ShiftModifier
                        const modifierOnly = [Qt.Key_Control, Qt.Key_Shift, Qt.Key_Alt, Qt.Key_Meta].includes(event.key)
                        const copy = ctrl && event.key === Qt.Key_C
                        if (!modifierOnly && !copy)
                            field.autoSelection = false
                        const recent = root.view === "tips" ? root.daemon.recent : []
                        if (event.key === Qt.Key_Escape) {
                            root.stepBack()
                        } else if (recent.length > 0 && (event.key === Qt.Key_Down || event.key === Qt.Key_Up)) {
                            const step = event.key === Qt.Key_Down ? 1 : -1
                            stateView.recentIndex = Math.max(-1, Math.min(recent.length - 1, stateView.recentIndex + step))
                        } else if (recent.length > 0 && stateView.recentIndex >= 0 && (event.key === Qt.Key_Return || event.key === Qt.Key_Enter)) {
                            root.searched = true
                            field.text = recent[stateView.recentIndex]
                        } else if (ctrl && event.key === Qt.Key_Comma) {
                            root.openSettings("", false)
                        } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                            root.act(ctrl ? "reveal" : "open")
                        } else if (ctrl && event.key >= Qt.Key_1 && event.key <= Qt.Key_3) {
                            root.setMode(root.modes[event.key - Qt.Key_1].id)
                        } else if (copy && root.results.current.path
                                   && (shift || field.selectedText === "" || field.autoSelection)) {
                            root.act(shift ? "snippet" : "path")
                        } else if (ctrl && !shift && event.key === Qt.Key_S) {
                            root.act("similar")
                        } else if (event.modifiers & (Qt.ControlModifier | Qt.AltModifier) || !root.results.key(event.key)) {
                            return
                        } else {
                            // Moving through results ends the "type to replace" selection, so
                            // that Ctrl+C copies the result's path.
                            field.deselect()
                        }
                        event.accepted = true
                    }

                    Kirigami.Icon {
                        id: searchIcon

                        anchors.left: parent.left
                        anchors.leftMargin: Kirigami.Units.largeSpacing
                        anchors.verticalCenter: parent.verticalCenter
                        width: Kirigami.Units.iconSizes.smallMedium
                        height: width
                        source: "search"
                        color: Kirigami.Theme.disabledTextColor
                    }

                    QQC2.ToolButton {
                        id: clearButton

                        anchors.right: parent.right
                        anchors.verticalCenter: parent.verticalCenter
                        visible: field.text !== ""
                        icon.name: "edit-clear"
                        focusPolicy: Qt.NoFocus
                        display: QQC2.AbstractButton.IconOnly
                        text: qsTr("清空")
                        onClicked: {
                            field.clear()
                            field.forceActiveFocus()
                        }
                    }
                }

                Row {
                    Repeater {
                        model: root.modes

                        QQC2.Button {
                            required property var modelData
                            required property int index

                            text: modelData.label
                            checkable: true
                            checked: root.mode === modelData.id
                            focusPolicy: Qt.NoFocus
                            onClicked: root.setMode(modelData.id)
                            QQC2.ToolTip.text: qsTr("%1（Ctrl+%2）").arg(modelData.tip).arg(index + 1)
                            QQC2.ToolTip.visible: hovered
                            QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay
                        }
                    }
                }

                QQC2.ToolButton {
                    objectName: "gear"
                    icon.name: "configure"
                    display: QQC2.AbstractButton.IconOnly
                    text: qsTr("设置（Ctrl+,）")
                    focusPolicy: Qt.NoFocus
                    onClicked: root.openSettings("", false)
                    QQC2.ToolTip.text: text
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay
                }
            }

            Banner {
                id: banner

                Layout.fillWidth: true
                Layout.leftMargin: Kirigami.Units.largeSpacing * 1.5
                Layout.rightMargin: Kirigami.Units.largeSpacing * 1.5
                Layout.bottomMargin: Kirigami.Units.largeSpacing
                daemon: root.daemon
                profile: root.settings.saved["resources.profile"] ?? ""
                onSettingsRequested: page => root.openSettings(page, false)
            }

            Kirigami.Separator {
                Layout.fillWidth: true
            }

            Item {
                id: body

                Layout.fillWidth: true
                Layout.fillHeight: true

                RowLayout {
                    anchors.fill: parent
                    spacing: 0
                    visible: root.view === "results"

                    ResultList {
                        id: list

                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        results: root.results
                        mode: root.results.mode
                        onActivated: root.act("open")
                        onContextMenu: menu.popup()
                        onClicked: field.forceActiveFocus()
                    }

                    Kirigami.Separator {
                        Layout.fillHeight: true
                    }

                    Preview {
                        Layout.preferredWidth: Math.round(root.width * 0.44)
                        Layout.fillHeight: true
                        current: root.results.current
                        preview: root.daemon.previewId !== "" ? root.daemon.preview : ({})
                        results: root.results
                        onAct: action => root.act(action)
                        onChunk: id => root.daemon.fetchPreview(id, root.results.query)
                    }
                }

                StateView {
                    id: stateView

                    anchors.fill: parent
                    visible: root.view !== "results"
                    view: root.view
                    query: field.text.trim()
                    mode: root.mode
                    daemon: root.daemon
                    onModeRequested: id => root.setMode(id)
                    onCopy: text => {
                        root.popup.copy(text)
                        root.toast(qsTr("已复制命令"))
                    }
                    onRerun: query => {
                        root.searched = true
                        field.text = query
                        field.forceActiveFocus()
                    }
                    onSettingsRequested: (page, welcome) => root.openSettings(page, welcome)
                }
            }

            Kirigami.Separator {
                Layout.fillWidth: true
            }

            StatusLine {
                id: status

                Layout.fillWidth: true
                daemon: root.daemon
                results: root.results
                modeLabel: root.modes.find(m => m.id === root.mode).label
                showCounts: root.view === "results"
            }
        }
    }
}
