pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

// 排除规则: ignore files, the builtin and sensitive lists, hidden files, the size limit, extra
// rules, and checking one path against the applied rules.
Kirigami.FormLayout {
    id: page

    required property Settings settings

    readonly property var reasons: ({
        outside_sources: qsTr("不在任何索引目录里"),
        internal: qsTr("这是 Endex 自己的数据，不索引"),
        symlink: qsTr("符号链接，不跟随"),
        special: qsTr("不是普通文件（设备、管道或套接字）"),
        hidden: qsTr("路径里有隐藏文件或目录"),
        sensitive: qsTr("命中敏感文件规则 %1"),
        builtin: qsTr("命中内置规则 %1"),
        rule: qsTr("命中自定义规则 %1"),
        gitignore: qsTr("被 %1 忽略（%2）"),
        unsupported: qsTr("不支持的文件格式"),
        kind: qsTr("所在目录设为只索引另一类文件"),
        encoding: qsTr("文件名不是有效的 UTF-8"),
        too_large: qsTr("超过单个文档大小上限"),
    })
    property var checked: ({})

    function describe(r) {
        if (r.error)
            return [false, qsTr("检查失败：%1").arg(r.error)]
        if (r.indexed) {
            const what = r.kind === "image" ? qsTr("图片") : r.kind === "doc" ? qsTr("文档") : qsTr("目录")
            return [true, qsTr("会被索引（%1，属于 %2）").arg(what).arg(r.source)]
        }
        let why = reasons[r.reason] ?? r.reason
        if (r.reason === "gitignore")
            why = why.arg(r.file).arg(r.rule)
        else if (["sensitive", "builtin", "rule"].includes(r.reason))
            why = why.arg(r.rule)
        return [false, qsTr("不会被索引：%1").arg(why)]
    }

    function rulesText() {
        return (settings.draft["exclude.rules"] ?? []).join("\n")
    }

    Connections {
        target: page.settings

        function onPathChecked(path, result) {
            page.checked = { path: path, result: result }
        }

        // Revert or reload: the rules box follows unless it is being edited.
        function onChanged() {
            if (!rules.activeFocus)
                rules.text = page.rulesText()
        }
    }

    Kirigami.Separator {
        Kirigami.FormData.isSection: true
        Kirigami.FormData.label: qsTr("忽略规则")
    }
    SettingSwitch {
        Kirigami.FormData.label: qsTr(".gitignore：")
        Layout.fillWidth: true
        settings: page.settings
        key: "exclude.gitignore"
        text: qsTr("遵守 .gitignore")
        hint: qsTr("目录里的 .gitignore 和 .ignore 生效；在 git 仓库里也看上级目录的规则（不读主目录本身的）。")
    }
    ColumnLayout {
        Kirigami.FormData.label: qsTr("内置排除：")
        Layout.fillWidth: true
        spacing: 0

        SettingSwitch {
            Layout.fillWidth: true
            settings: page.settings
            key: "exclude.builtin"
            text: qsTr("排除常见的构建产物和缓存")
            hint: qsTr("一份内置清单，例如 node_modules、target、.git。")
        }
        PatternList {
            label: qsTr("查看内置清单")
            patterns: page.settings.saved["exclude.builtin_list"] ?? []
        }
    }
    ColumnLayout {
        Kirigami.FormData.label: qsTr("敏感文件：")
        Layout.fillWidth: true
        spacing: 0

        SettingSwitch {
            Layout.fillWidth: true
            settings: page.settings
            key: "exclude.sensitive"
            text: qsTr("不读取疑似密钥和凭据的文件")
            hint: qsTr("命中的文件连内容都不会读取。")
        }
        PatternList {
            label: qsTr("查看规则")
            patterns: page.settings.saved["exclude.sensitive_list"] ?? []
        }
    }
    SettingSwitch {
        Kirigami.FormData.label: qsTr("隐藏文件：")
        Layout.fillWidth: true
        settings: page.settings
        key: "exclude.hidden"
        text: qsTr("索引以 . 开头的文件和目录")
        hint: qsTr("默认关闭。")
    }

    Kirigami.Separator {
        Kirigami.FormData.isSection: true
        Kirigami.FormData.label: qsTr("大小")
    }
    ColumnLayout {
        Kirigami.FormData.label: qsTr("单个文档上限：")
        Layout.fillWidth: true
        spacing: 0

        RowLayout {
            SettingSpin {
                settings: page.settings
                key: "index.max_document_mb"
                from: 1
                to: 1024
            }
            QQC2.Label {
                textFormat: Text.PlainText
                text: qsTr("MB")
            }
        }
        Hint {
            text: qsTr("超过上限的文档会被跳过，并列在「索引状态」的失败文件里。默认 10。")
        }
    }

    Kirigami.Separator {
        Kirigami.FormData.isSection: true
        Kirigami.FormData.label: qsTr("自定义")
    }
    ColumnLayout {
        Kirigami.FormData.label: qsTr("额外排除规则：")
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing

        QQC2.TextArea {
            id: rules

            objectName: "exclude.rules"
            Layout.fillWidth: true
            Layout.minimumHeight: Kirigami.Units.gridUnit * 5
            font.family: "monospace"
            placeholderText: "*.log\nbackup/"
            text: page.rulesText()
            onTextChanged: {
                if (activeFocus)
                    page.settings.set("exclude.rules", text.split("\n"))
            }
        }
        Hint {
            text: qsTr("每行一条，写法同 .gitignore，相对于每个索引目录。")
        }
        Repeater {
            model: Object.keys(page.settings.errors).filter(k => k.startsWith("exclude.rules."))

            Hint {
                required property string modelData

                color: Kirigami.Theme.negativeTextColor
                text: qsTr("第 %1 条：%2").arg(Number(modelData.split(".")[2]) + 1).arg(page.settings.errors[modelData])
            }
        }
    }
    ColumnLayout {
        Kirigami.FormData.label: qsTr("检查一个路径：")
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            Layout.fillWidth: true

            QQC2.TextField {
                id: probe

                objectName: "checkPathField"
                Layout.fillWidth: true
                font.family: "monospace"
                placeholderText: qsTr("~/某个目录/某个文件")
                onAccepted: page.settings.checkPath(text)
            }
            QQC2.Button {
                objectName: "checkPath"
                text: qsTr("检查")
                enabled: probe.text.trim() !== ""
                onClicked: page.settings.checkPath(probe.text)
            }
        }
        QQC2.Label {
            readonly property var verdict: page.checked.path === probe.text && page.checked.result ? page.describe(page.checked.result) : null

            objectName: "checkPathResult"
            Layout.fillWidth: true
            Layout.maximumWidth: Kirigami.Units.gridUnit * 30
            visible: verdict !== null
            textFormat: Text.PlainText
            wrapMode: Text.Wrap
            text: verdict ? verdict[1] : ""
            color: verdict && verdict[0] ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.negativeTextColor
        }
        Hint {
            visible: page.settings.pending.length > 0
            text: qsTr("按已经应用的设置检查；还没应用的改动不算在内。")
        }
    }

    component PatternList: ColumnLayout {
        id: list

        property string label
        property var patterns: []

        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing

        QQC2.ToolButton {
            id: expand

            text: list.label
            icon.name: expand.checked ? "arrow-down" : "arrow-right"
            checkable: true
        }
        Flow {
            Layout.fillWidth: true
            visible: expand.checked
            spacing: Kirigami.Units.smallSpacing

            Repeater {
                model: list.patterns

                KeyCap {
                    required property string modelData

                    text: modelData
                }
            }
        }
    }
}
