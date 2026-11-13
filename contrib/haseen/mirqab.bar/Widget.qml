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

    // The item and interval the reading runs with, set once the widget is
    // built: haseen assigns the settings right after creation, and that
    // must not restart anything.
    property string started: ""
    property bool ready: false

    function key() {
        return String(settings.item || "combo") + " " + String(settings.interval || 5);
    }

    Component.onCompleted: {
        started = key();
        ready = true;
    }

    // A changed item or interval restarts the reading, by the timer, so it
    // does not depend on an exit being delivered.
    onSettingsChanged: {
        if (!ready || key() === started)
            return;
        started = key();
        watcher.running = false;
        restart.interval = 100;
        restart.restart();
    }

    onClicked: button => {
        if (button === Qt.LeftButton)
            Quickshell.execDetached(["mirqab", "toggle"]);
    }

    Process {
        id: watcher

        command: ["mirqab", "bar", String(root.settings.item || "combo"), "--watch", "--interval", String(root.settings.interval || 5), "--format", "json"]
        running: true
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
        // again in 10 s, unless a settings restart is already due.
        onExited: {
            root.reading = "\u2014";
            root.level = "unknown";
            if (!restart.running) {
                restart.interval = 10000;
                restart.start();
            }
        }
    }

    Timer {
        id: restart

        interval: 10000
        onTriggered: watcher.running = true
    }
}
