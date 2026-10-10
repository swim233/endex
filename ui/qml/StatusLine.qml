import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Endex

RowLayout {
    id: line

    required property Daemon daemon
    required property Results results
    property string modeLabel
    property bool showCounts

    function toast(message) {
        toastLabel.text = message
        toastTimer.restart()
    }

    function number(n) {
        return Number(n ?? 0).toLocaleString(Qt.locale(), "f", 0)
    }

    spacing: Kirigami.Units.largeSpacing

    Timer {
        id: toastTimer
        interval: 2400
    }

    Rectangle {
        Layout.leftMargin: Kirigami.Units.largeSpacing * 1.5
        Layout.preferredWidth: Kirigami.Units.smallSpacing * 2
        Layout.preferredHeight: Kirigami.Units.smallSpacing * 2
        radius: width / 2
        color: {
            switch (line.daemon.state) {
            case Daemon.Ready:
                return line.daemon.searchError ? Kirigami.Theme.neutralTextColor : Kirigami.Theme.positiveTextColor
            case Daemon.Down:
            case Daemon.Broken:
                return Kirigami.Theme.negativeTextColor
            default:
                return Kirigami.Theme.neutralTextColor
            }
        }
    }

    QQC2.Label {
        textFormat: Text.PlainText
        Layout.topMargin: Kirigami.Units.smallSpacing * 1.5
        Layout.bottomMargin: Kirigami.Units.smallSpacing * 1.5
        font: Kirigami.Theme.smallFont
        text: {
            const s = line.daemon.status
            switch (line.daemon.state) {
            case Daemon.Ready:
                if (line.daemon.searchError)
                    return qsTr("搜索出错：%1").arg(line.daemon.searchError)
                return qsTr("索引就绪 · %1 个文件 · %2 个片段")
                    .arg(line.number(s.doc_files + s.image_files)).arg(line.number(s.doc_chunks + s.image_chunks))
            case Daemon.Empty:
                return qsTr("还没有索引")
            case Daemon.Broken:
                return qsTr("索引不可用")
            case Daemon.Down:
                return qsTr("守护进程未运行")
            case Daemon.Starting:
                return qsTr("正在启动守护进程…")
            case Daemon.Rebuilding:
                return qsTr("正在重建索引…")
            default:
                return qsTr("正在连接守护进程…")
            }
        }
        elide: Text.ElideRight
        Layout.maximumWidth: line.width * 0.4
    }

    QQC2.Label {
        textFormat: Text.PlainText
        visible: line.showCounts && line.daemon.state === Daemon.Ready
        font: Kirigami.Theme.smallFont
        text: line.daemon.searching ? qsTr("搜索中…")
            : qsTr("%1 文档 · %2 图片 · %3 s").arg(line.results.docCount).arg(line.results.imageCount)
                .arg((line.daemon.latency / 1000).toFixed(2))
    }

    QQC2.Label {
        textFormat: Text.PlainText
        visible: line.daemon.state === Daemon.Ready
        font: Kirigami.Theme.smallFont
        text: line.modeLabel
    }

    QQC2.Label {
        textFormat: Text.PlainText
        visible: line.daemon.state === Daemon.Ready
        font: Kirigami.Theme.smallFont
        text: qsTr("查询设备 %1").arg(String(line.daemon.status.query_device ?? "cpu").toUpperCase())
        color: Kirigami.Theme.disabledTextColor
    }

    Item {
        Layout.fillWidth: true
    }

    QQC2.Label {
        id: toastLabel

        textFormat: Text.PlainText
        Layout.rightMargin: Kirigami.Units.largeSpacing * 1.5
        Layout.maximumWidth: line.width * 0.45
        visible: toastTimer.running
        font: Kirigami.Theme.smallFont
        color: Kirigami.Theme.positiveTextColor
        elide: Text.ElideMiddle
    }

    QQC2.Label {
        textFormat: Text.PlainText
        Layout.rightMargin: Kirigami.Units.largeSpacing * 1.5
        visible: !toastTimer.running
        font: Kirigami.Theme.smallFont
        color: Kirigami.Theme.disabledTextColor
        text: qsTr("↑↓ 选择 · Tab 分组 · PgUp/PgDn 翻页 · ↵ 打开 · Esc 关闭")
    }
}
