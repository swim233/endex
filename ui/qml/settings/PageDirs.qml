pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

// 索引目录: the source folders, the suggested ones to decide on, and adding a folder.
ColumnLayout {
    id: page

    required property Daemon daemon
    required property Settings settings
    required property bool welcome
    // A read-only daemon applies nothing.
    required property bool writable

    signal addRequested()

    readonly property var sources: settings.draft["index.sources"] ?? []
    readonly property var declined: settings.draft["index.declined"] ?? []
    readonly property var savedSources: settings.saved["index.sources"] ?? []
    readonly property var suggested: (settings.saved["index.suggested"] ?? [])
        .filter(p => !declined.includes(p) && !sources.some(s => s.path === p))
    readonly property var kinds: [["all", qsTr("文档和图片")], ["docs", qsTr("仅文档")], ["images", qsTr("仅图片")]]

    function number(n) {
        return Number(n ?? 0).toLocaleString(Qt.locale(), "f", 0)
    }

    function counted(path) {
        return (daemon.status.sources ?? []).find(s => s.path === path)
    }

    function setKind(index, kind) {
        settings.set("index.sources", sources.map((s, i) => i === index ? { path: s.path, kind: kind } : s))
    }

    function remove(index) {
        settings.set("index.sources", sources.filter((s, i) => i !== index))
    }

    function accept(path) {
        settings.set("index.sources", sources.concat([{ path: path, kind: "all" }]))
    }

    function decline(path, declining) {
        settings.set("index.declined", declining ? declined.concat([path]) : declined.filter(p => p !== path))
    }

    spacing: Kirigami.Units.largeSpacing

    Notice {
        Layout.fillWidth: true
        visible: page.welcome
        type: Kirigami.MessageType.Positive
        text: qsTr("<b>欢迎使用 Endex</b>：先添加要索引的目录，点「应用」开始首次索引；之后按你绑定的快捷键（见「快捷键指引」）即可体验搜索。")
    }
    Notice {
        Layout.fillWidth: true
        visible: page.writable
        text: qsTr("索引将在设置应用后进行")
    }

    Card {
        spacing: Kirigami.Units.smallSpacing

        Repeater {
            model: page.sources

            delegate: RowLayout {
                id: row

                required property var modelData
                required property int index
                readonly property bool applied: page.savedSources.some(s => s.path === modelData.path)
                readonly property var counts: page.counted(modelData.path)
                readonly property string error: page.settings.errors["index.sources." + index] ?? ""

                Layout.fillWidth: true
                spacing: Kirigami.Units.largeSpacing

                Kirigami.Icon {
                    source: row.counts && !row.counts.exists ? "folder-important" : "folder"
                    implicitWidth: Kirigami.Units.iconSizes.medium
                    implicitHeight: implicitWidth
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0

                    QQC2.Label {
                        Layout.fillWidth: true
                        textFormat: Text.PlainText
                        text: row.modelData.path
                        elide: Text.ElideMiddle
                    }
                    Hint {
                        visible: row.error === ""
                        color: row.counts && !row.counts.exists ? Kirigami.Theme.neutralTextColor : Kirigami.Theme.disabledTextColor
                        text: {
                            if (!row.applied)
                                return qsTr("点「应用」后开始索引")
                            const c = row.counts
                            if (!c)
                                return ""
                            const files = qsTr("%1 个文件").arg(page.number(c.files))
                            const failed = c.errors > 0 ? qsTr(" · %1 个处理失败").arg(page.number(c.errors)) : ""
                            if (!c.exists)
                                return qsTr("%1 · 暂时不可用（文件仍保留在索引中）").arg(files)
                            return files + failed
                        }
                    }
                    Hint {
                        visible: row.error !== ""
                        color: Kirigami.Theme.negativeTextColor
                        text: row.error
                    }
                }
                QQC2.ComboBox {
                    objectName: "kind." + row.index
                    model: page.kinds.map(k => k[1])
                    currentIndex: page.kinds.findIndex(k => k[0] === row.modelData.kind)
                    onActivated: index => page.setKind(row.index, page.kinds[index][0])
                }
                QQC2.Button {
                    objectName: "remove." + row.index
                    text: qsTr("移除")
                    icon.name: "edit-delete-remove"
                    onClicked: page.remove(row.index)
                }
            }
        }

        Hint {
            visible: page.sources.length === 0
            text: qsTr("还没有目录。")
        }
    }

    QQC2.Label {
        visible: page.suggested.length + page.declined.length > 0
        textFormat: Text.PlainText
        text: qsTr("需要你决定")
        font.bold: true
    }
    Card {
        visible: page.suggested.length + page.declined.length > 0
        spacing: Kirigami.Units.smallSpacing

        Repeater {
            model: page.suggested

            delegate: RowLayout {
                id: undecided

                required property string modelData

                Layout.fillWidth: true
                spacing: Kirigami.Units.largeSpacing

                Kirigami.Icon {
                    source: "object-locked"
                    implicitWidth: Kirigami.Units.iconSizes.medium
                    implicitHeight: implicitWidth
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0

                    QQC2.Label {
                        Layout.fillWidth: true
                        textFormat: Text.PlainText
                        text: undecided.modelData
                        elide: Text.ElideMiddle
                    }
                    Hint {
                        text: qsTr("尚未被索引")
                    }
                }
                QQC2.Button {
                    objectName: "accept." + undecided.modelData
                    text: qsTr("纳入索引")
                    highlighted: true
                    onClicked: page.accept(undecided.modelData)
                }
                QQC2.Button {
                    objectName: "decline." + undecided.modelData
                    text: qsTr("不索引")
                    onClicked: page.decline(undecided.modelData, true)
                }
            }
        }

        Repeater {
            model: page.declined

            delegate: RowLayout {
                id: declinedRow

                required property string modelData

                Layout.fillWidth: true
                spacing: Kirigami.Units.largeSpacing
                opacity: 0.8

                Kirigami.Icon {
                    source: "folder-locked"
                    implicitWidth: Kirigami.Units.iconSizes.medium
                    implicitHeight: implicitWidth
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0

                    QQC2.Label {
                        Layout.fillWidth: true
                        textFormat: Text.PlainText
                        text: declinedRow.modelData
                        elide: Text.ElideMiddle
                    }
                    Hint {
                        text: qsTr("已排除索引")
                    }
                }
                QQC2.Button {
                    objectName: "undecline." + declinedRow.modelData
                    text: qsTr("撤销")
                    flat: true
                    onClicked: page.decline(declinedRow.modelData, false)
                }
            }
        }
    }

    RowLayout {
        spacing: Kirigami.Units.largeSpacing

        QQC2.Button {
            objectName: "addFolder"
            text: qsTr("添加目录…")
            icon.name: "folder-new"
            onClicked: page.addRequested()
        }
        Hint {
            visible: page.savedSources.length > 0
            text: qsTr("总计 %1 个文件").arg(page.number((page.daemon.status.sources ?? []).reduce((n, s) => n + s.files, 0)))
        }
    }
}
