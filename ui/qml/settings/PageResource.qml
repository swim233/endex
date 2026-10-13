pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

// 性能: battery, priority, threads per profile, GPU release.
Kirigami.FormLayout {
    id: page

    required property Daemon daemon
    required property Settings settings

    readonly property var status: daemon.status
    readonly property var profiles: [
        ["light", qsTr("轻"), qsTr("索引更慢，节约性能")],
        ["mid", qsTr("中"), qsTr("日常使用推荐")],
        ["heavy", qsTr("重"), qsTr("尽快建立索引，可能占用较多性能")],
    ]

    Kirigami.Separator {
        Kirigami.FormData.isSection: true
        Kirigami.FormData.label: qsTr("电池")
    }
    ColumnLayout {
        Kirigami.FormData.label: qsTr("电池供电时：")
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing

        SettingSwitch {
            Layout.fillWidth: true
            settings: page.settings
            key: "resources.pause_on_battery"
            text: qsTr("暂停索引")
            hint: qsTr("暂停索引，等待电源重新接入")
        }
        RowLayout {
            spacing: Kirigami.Units.largeSpacing

            Hint {
                Layout.fillWidth: false
                text: {
                    if (!page.status.battery_known)
                        return qsTr("读不到电源状态（UPower）。")
                    if (!page.status.on_battery)
                        return qsTr("现在：接着电源。")
                    const percent = page.status.battery_percent >= 0 ? qsTr("（剩余 %1%）").arg(Math.round(page.status.battery_percent)) : ""
                    return qsTr("现在：电池供电%1。").arg(percent)
                        + (page.status.battery_override ? qsTr("本次已选择继续索引，直到电源状态改变。") : "")
                }
            }
            QQC2.Button {
                objectName: "continueOnBatteryResource"
                visible: page.daemon.indexState.state === "paused" && page.daemon.indexState.reason === "battery"
                text: qsTr("本次继续")
                icon.name: "media-playback-start"
                onClicked: page.daemon.control("ContinueOnBattery")
            }
        }
    }

    Kirigami.Separator {
        Kirigami.FormData.isSection: true
        Kirigami.FormData.label: qsTr("优先级与线程")
    }
    SettingSwitch {
        Kirigami.FormData.label: qsTr("低优先级：")
        Layout.fillWidth: true
        settings: page.settings
        key: "resources.low_priority"
        text: qsTr("以低优先级运行索引")
        hint: qsTr("降低处理器和磁盘优先级，重启守护进程后生效。")
    }
    ColumnLayout {
        Kirigami.FormData.label: qsTr("线程档位：")
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing

        SettingChoice {
            settings: page.settings
            key: "resources.profile"
            options: page.profiles.map(p => [p[0], p[1]])
        }
        GridLayout {
            columns: 3
            columnSpacing: Kirigami.Units.largeSpacing
            rowSpacing: Kirigami.Units.smallSpacing

            Repeater {
                model: page.profiles

                delegate: QQC2.Label {
                    required property var modelData
                    required property int index

                    Layout.row: index
                    Layout.column: 0
                    textFormat: Text.PlainText
                    text: modelData[1]
                    font.bold: page.settings.draft["resources.profile"] === modelData[0]
                }
            }
            Repeater {
                model: page.profiles

                delegate: RowLayout {
                    id: threads

                    required property var modelData
                    required property int index

                    Layout.row: index
                    Layout.column: 1

                    SettingSpin {
                        settings: page.settings
                        key: "resources.threads." + threads.modelData[0]
                        from: 1
                        to: 64
                    }
                    QQC2.Label {
                        textFormat: Text.PlainText
                        text: qsTr("线程")
                    }
                }
            }
            Repeater {
                model: page.profiles

                delegate: Hint {
                    required property var modelData
                    required property int index

                    Layout.row: index
                    Layout.column: 2
                    text: modelData[2]
                }
            }
        }
        Hint {
            text: qsTr("线程数是识别图片文字（OCR）和 CPU 嵌入的上限。OCR 在「中」档每张图约 1.2–4 秒；第一次给大量截图建索引时，可以临时改用「重」，建完再改回来。")
        }
    }

    Kirigami.Separator {
        Kirigami.FormData.isSection: true
        Kirigami.FormData.label: qsTr("GPU")
    }
    ColumnLayout {
        Kirigami.FormData.label: qsTr("空闲后释放：")
        Layout.fillWidth: true
        spacing: 0

        RowLayout {
            SettingSpin {
                settings: page.settings
                key: "resources.gpu_idle_release_min"
                from: 0
                to: 1440
            }
            QQC2.Label {
                textFormat: Text.PlainText
                text: qsTr("分钟未使用 GPU 后释放显存")
            }
        }
        Hint {
            text: qsTr("设置为 0 时候不再自动释放显存")
        }
    }
}
