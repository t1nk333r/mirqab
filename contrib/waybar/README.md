# mirqab in Waybar

`mirqab bar <item> --watch` prints one line of Waybar's custom-module
JSON (`text`, `alt`, `tooltip`, `class`, `percentage`) each time the
reading changes, at most once per interval (5 s by default, at least 2 s).
It holds one non-live subscription to the daemon, and ends when Waybar
closes its output. The text carries no colour or markup: style the
`class` in Waybar's CSS.

Add any of these modules to your Waybar config and name them in a
`modules-*` list. `on-click` toggles mirqab's window on the matching tab.

```json
{
  "custom/mirqab-cpu": {
    "exec": "mirqab bar cpu --watch",
    "return-type": "json",
    "format": "CPU {}",
    "on-click": "mirqab toggle system"
  },
  "custom/mirqab-mem": {
    "exec": "mirqab bar mem --watch",
    "return-type": "json",
    "format": "MEM {}",
    "on-click": "mirqab toggle system"
  },
  "custom/mirqab-temp": {
    "exec": "mirqab bar temp --watch",
    "return-type": "json",
    "format": "{}",
    "on-click": "mirqab toggle system"
  },
  "custom/mirqab-gpu": {
    "exec": "mirqab bar gpu --watch",
    "return-type": "json",
    "format": "GPU {}",
    "on-click": "mirqab toggle system"
  },
  "custom/mirqab-load": {
    "exec": "mirqab bar load --watch",
    "return-type": "json",
    "format": "{}",
    "on-click": "mirqab toggle system"
  },
  "custom/mirqab": {
    "exec": "mirqab bar combo --watch --interval 5",
    "return-type": "json",
    "format": "{}",
    "on-click": "mirqab toggle"
  }
}
```

Classes: `cpu`, `mem` and `gpu` are `normal`, `high` (80 % or more) or
`critical` (95 % or more); `temp` is the thermal word (`cool`, `warm`,
`hot`, `critical`); `combo` is the worst of its parts; a reading with no
value yet is `unknown`. For example:

```css
#custom-mirqab.high, #custom-mirqab-cpu.high { color: @warning_color; }
#custom-mirqab.critical, #custom-mirqab-cpu.critical { color: @error_color; }
```

The fields were checked against Waybar's wiki
(https://github.com/Alexays/Waybar/wiki/Module:-Custom); Waybar was not
installed on the development machine.
