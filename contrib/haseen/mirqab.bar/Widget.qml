import QtQuick
import Quickshell
import Quickshell.Io
import qs.Haseen
import qs.Haseen.Widgets

// mirqab's reading in haseen's bar (mirqab plan 004). It runs
// `mirqab bar <item> --watch --format json`, which prints a line only when
// the reading changes, and shows the text; the class colours it. A click
// toggles mirqab's window. Written for mirqab; it shares no code with
// mirqab's GTK interface.
BarButton {
    id: root

    // Contract (haseen architecture 5.2): every entry component gets these.
    property string pluginId
    property var settings: ({})
    property var screen: null

    property string reading: ""
    property string level: "normal"

    text: reading
    visible: reading !== ""
    implicitWidth: visible ? contentWidth : 0
    color: level === "critical" ? Theme.urgent : (level === "high" || level === "hot") ? Theme.warning : Theme.barForeground

    // The item and interval the running command was started with.
    property string started: ""

    function key() {
        return String(settings.item || "combo") + " " + String(settings.interval || 5);
    }

    // A changed item or interval restarts the reading: the command is
    // bound once per run. The first assignment at creation changes nothing.
    onSettingsChanged: {
        if (key() === started)
            return;
        started = key();
        if (watcher.running) {
            restart.settingsChange = true;
            watcher.running = false;
        } else {
            restart.stop();
            watcher.running = true;
        }
    }

    onClicked: button => {
        if (button === Qt.LeftButton)
            Quickshell.execDetached(["mirqab", "toggle"]);
    }

    Process {
        id: watcher

        command: ["mirqab", "bar", String(root.settings.item || "combo"), "--watch", "--interval", String(root.settings.interval || 5), "--format", "json"]
        running: true
        onStarted: root.started = root.key()
        stdout: SplitParser {
            onRead: line => {
                try {
                    const r = JSON.parse(line);
                    root.reading = r.text || "";
                    root.level = r["class"] || "normal";
                } catch (e) {
                    root.reading = "";
                }
            }
        }
        // The daemon stopped or is not up yet: no stale number, and try
        // again shortly; a settings change restarts at once.
        onExited: {
            root.reading = "\u2014";
            root.level = "unknown";
            restart.interval = restart.settingsChange ? 100 : 10000;
            restart.settingsChange = false;
            restart.start();
        }
    }

    Timer {
        id: restart

        property bool settingsChange: false

        interval: 10000
        onTriggered: watcher.running = true
    }
}
