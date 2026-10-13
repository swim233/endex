pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Dialogs
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

// 模型与设备: the embedding model, devices per use, prefix style, OCR.
Kirigami.FormLayout {
    id: page

    required property Daemon daemon
    required property Settings settings

    readonly property var devices: [["cpu", qsTr("CPU")], ["gpu", qsTr("GPU（WebGPU / Vulkan）")]]
    readonly property string model: settings.draft["embedding.model"] ?? ""
    readonly property bool modelFound: settings.isModelFile(model)
    // These change what the index holds: applying one needs a rebuild.
    readonly property bool identityEdited: settings.pending.some(k => ["embedding.model", "embedding.prefix_style", "ocr.enabled"].includes(k))
    readonly property var indexState: daemon.indexState

    // Revert or reload: the path field follows unless it is being edited.
    Connections {
        target: page.settings

        function onChanged() {
            if (!modelField.activeFocus)
                modelField.text = page.model
        }
    }

    FileDialog {
        id: modelDialog

        title: qsTr("选择嵌入模型")
        nameFilters: [qsTr("LiteRT-LM 模型 (*.litertlm)")]
        onAccepted: page.settings.set("embedding.model", page.settings.configPath(selectedFile))
    }

    Notice {
        Kirigami.FormData.isSection: true
        Layout.fillWidth: true
        visible: page.identityEdited
        type: Kirigami.MessageType.Warning
        text: qsTr("应用后，已有索引要重建才会用上新的模型设置（「索引状态」里的「重建索引」）。")
    }

    Kirigami.Separator {
        Kirigami.FormData.isSection: true
        Kirigami.FormData.label: qsTr("嵌入模型")
    }
    ColumnLayout {
        Kirigami.FormData.label: qsTr("模型路径：")
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            Layout.fillWidth: true

            QQC2.TextField {
                id: modelField

                objectName: "embedding.model"
                Layout.fillWidth: true
                Layout.minimumWidth: Kirigami.Units.gridUnit * 18
                font.family: "monospace"
                text: page.model
                onTextEdited: page.settings.set("embedding.model", text)
            }
            QQC2.Button {
                text: qsTr("浏览…")
                icon.name: "document-open"
                onClicked: modelDialog.open()
            }
        }
        QQC2.Label {
            Layout.fillWidth: true
            Layout.maximumWidth: Kirigami.Units.gridUnit * 30
            textFormat: Text.PlainText
            wrapMode: Text.Wrap
            color: page.modelFound ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.negativeTextColor
            text: {
                const error = page.settings.errors["embedding.model"]
                if (error)
                    return error
                if (!page.modelFound)
                    return qsTr("找不到模型，或不是 .litertlm 文件")
                if (page.model === page.settings.saved["embedding.model"] && page.daemon.status.model)
                    return qsTr("已找到 · 正在使用 %1").arg(page.daemon.status.model)
                return qsTr("已找到")
            }
        }
    }

    Kirigami.Separator {
        Kirigami.FormData.isSection: true
        Kirigami.FormData.label: qsTr("运行设备")
    }
    ColumnLayout {
        Kirigami.FormData.label: qsTr("查询：")
        Layout.fillWidth: true
        spacing: 0

        SettingCombo {
            settings: page.settings
            key: "devices.query"
            options: page.devices
        }
        Hint {
            text: qsTr("建议使用 CPU 设备，降低性能占用")
        }
    }
    ColumnLayout {
        Kirigami.FormData.label: qsTr("批量索引：")
        Layout.fillWidth: true
        spacing: 0

        SettingCombo {
            settings: page.settings
            key: "devices.bulk"
            options: page.devices
        }
        Hint {
            text: qsTr("使用 GPU 设备以加速索引")
        }
    }
    ColumnLayout {
        Kirigami.FormData.label: qsTr("少量增量更新：")
        Layout.fillWidth: true
        spacing: 0

        SettingCombo {
            settings: page.settings
            key: "devices.incremental"
            options: page.devices
        }
        Hint {
            text: qsTr("建议使用 CPU 设备，降低性能占用")
        }
    }
    QQC2.Label {
        Kirigami.FormData.label: qsTr("GPU 状态：")
        textFormat: Text.PlainText
        text: {
            if (!page.indexState.gpu_loaded)
                return qsTr("当前未载入")
            if (page.indexState.device === "gpu" && ["indexing", "scanning"].includes(page.indexState.state))
                return qsTr("已载入，正在批量索引")
            return qsTr("已载入（空闲 %1 分钟后释放）").arg(page.settings.saved["resources.gpu_idle_release_min"] ?? 2)
        }
    }

    Kirigami.Separator {
        Kirigami.FormData.isSection: true
        Kirigami.FormData.label: qsTr("查询与文档前缀")
    }
    ColumnLayout {
        Kirigami.FormData.label: qsTr("前缀风格：")
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing

        SettingChoice {
            settings: page.settings
            key: "embedding.prefix_style"
            options: [["litert", "litert"], ["model_card", "model_card"]]
        }
        Hint {
            text: qsTr("litert 沿用 LiteRT-LM 示例里的前缀写法；model_card 按模型卡片上的写法。更换后已有向量要重建。")
        }
    }

    Kirigami.Separator {
        Kirigami.FormData.isSection: true
        Kirigami.FormData.label: qsTr("图片文字")
    }
    SettingSwitch {
        Kirigami.FormData.label: qsTr("OCR：")
        Layout.fillWidth: true
        settings: page.settings
        key: "ocr.enabled"
        text: qsTr("启用 OCR（PP-OCRv6）")
        hint: qsTr("开启后将允许对图片进行关键词搜索，更改此项设置需要重建索引")
    }

    Kirigami.Separator {
        Kirigami.FormData.isSection: true
        Kirigami.FormData.label: qsTr("重排模型")
    }
    QQC2.Label {
        Kirigami.FormData.label: qsTr("重排：")
        textFormat: Text.PlainText
        enabled: false
        text: qsTr("Qwen3-Reranker 重排前 30 条文本结果（M4 提供）")
    }
}
