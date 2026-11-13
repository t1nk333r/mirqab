# mirqab.bar: mirqab in haseen's bar

A haseen bar widget that shows one `mirqab bar` reading (by default
`combo`: CPU, memory and temperature) and toggles mirqab's window on a
left click. It runs `mirqab bar <item> --watch --format json`, which holds
one non-live subscription (5 s by default) and prints only when the
reading changes. High and critical readings take haseen's warning and
urgent colours.

It is not bundled with haseen and installing it does not enable it:

```sh
haseen plugin install /path/to/mirqab/contrib/haseen/mirqab.bar
haseen plugin validate mirqab.bar
```

Then enable it and place it in the bar from haseen's settings.

Settings (`shell.json`, `plugins.mirqab.bar.settings`):

| Setting | Default | Meaning |
|---------|---------|---------|
| `item` | `combo` | The item: `cpu`, `mem`, `temp`, `gpu`, `load` or `combo` |
| `interval` | `5` | Seconds between readings; mirqab raises anything under 2 |

Which parts `combo` shows is mirqab's setting `bar.combo`.
