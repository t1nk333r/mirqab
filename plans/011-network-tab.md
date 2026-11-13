# Plan 011: Network tab

## Status

- **Priority**: P2
- **Effort**: L, in three phases
- **Risk**: MEDIUM. This is the tab that can leak: addresses, network
  names and nearby devices are personal data, and three tools make
  outbound requests. Each is user-started or opt-in.
- **Depends on**: 001
- **Category**: tab
- **Planned at**: 2026-11-13, from the owner
- **State**: PLANNED

## Problem

Stat Space's Network tab shows live up and down speed, local and public
address, Wi-Fi link details, a speed test, ping, traceroute, DNS lookup, a
captive-portal check, an opt-in latency monitor, devices on the local
network, and (Pro) traffic by app
(https://statspace.app/features/system-monitor, "Network toolkit", and the
site's security page, read 2026-11-13).

## Linux data sources

| Data | Source | Root? |
|------|--------|-------|
| Bytes, packets, errors per interface | `/proc/net/dev` | no |
| Interface kind and state | `/sys/class/net/<if>/{operstate,type,speed,carrier}`; `wireless/` or `phy80211` directory means Wi-Fi; `bridge/` means bridge; `tun_flags` means tunnel; name prefixes `veth`, `docker`, `br-` for container plumbing. The development machine has Ethernet, Wi-Fi, bridges, container and VPN interfaces **[machine]** | no |
| Addresses | `getifaddrs` | no |
| Default route | `/proc/net/route` (IPv4), `/proc/net/ipv6_route` | no |
| DNS servers | `resolvectl` is present **[machine]**; D-Bus `org.freedesktop.resolve1` is on the bus **[machine]**; fallback `/etc/resolv.conf` | no |
| Wi-Fi link | NetworkManager D-Bus (`org.freedesktop.NetworkManager` on the bus **[machine]**, 1.58.1): active connection → `AccessPoint` `Ssid`, `Strength`, `Frequency`, `MaxBitrate`; device `Bitrate`. Fallback `iw dev <if> link` (`iw` present **[machine]**; iwd is not installed) | no |
| Latency | An unprivileged ICMP echo socket (`socket(AF_INET, SOCK_DGRAM, IPPROTO_ICMP)`); `net.ipv4.ping_group_range` is `0 2147483647` **[machine]**. Fallback: the `ping` binary (present) | no |
| Traceroute | UDP probes with rising TTL and `IP_RECVERR` on an unprivileged socket (how `traceroute` works without root, `ip(7)`); the `traceroute` binary is not installed **[machine]** | no |
| DNS lookup | `resolvectl query <name>` or `getaddrinfo` | no |
| Captive portal | NetworkManager's `Connectivity` property (it already checks); no request of our own | no |
| LAN devices | The kernel neighbour table (`ip -j neigh`, `ip` present) and mDNS names from Avahi (`org.freedesktop.Avahi` on the bus **[machine]**) | no |
| Per-app traffic | `sock_diag` netlink with `INET_DIAG_INFO` gives `tcpi_bytes_acked` and `tcpi_bytes_received` per TCP socket (`sock_diag(7)`), joined to pids by inode as in plan 007 | no, for the user's own sockets |
| Public address | A request to a keyless "what is my IP" service | no |
| Speed test | Timed download and upload with `curl` | no |

## Not possible on Linux

| Stat Space item | Decision |
|-----------------|----------|
| Traffic by app, all traffic | Limited: TCP only, own processes only, sampled (short connections are missed). UDP and QUIC are not counted. The page states this. Complete accounting needs eBPF or root: dropped |
| "Who's phoning home" alerts | Dropped (architecture §1.2) |
| Wi-Fi details without NetworkManager or `iw` | Absent |
| Active LAN scan | Dropped by choice: passive only (neighbours the kernel already knows, and mDNS). No ARP sweep, no port scan |

## Design

### Privacy defaults

- The public address is fetched only when the user presses "Show", is
  shown until the page closes, and is never stored or logged.
- SSIDs, MAC addresses and LAN device names are shown in the page and
  **never** written to history, events, logs, the health report or CLI
  output unless the user asks for that listing (`mirqab net --json` prints
  them; it is the user's own terminal).
- History stores bytes per interface under a neutral key: the interface
  *kind* and an index (`net.wifi0`, `net.eth0`, `net.vpn0`), not its name.
- Latency monitoring is off by default. Its target is a setting; the
  default suggestion is the default gateway (nothing leaves the LAN).
- Every external endpoint is a setting, shown next to its button. **No
  speed-test and no public-address endpoint ships** (owner, 2026-11-13):
  `net.speedtest.url` and `net.publicIp.url` are empty by default, and
  while one is empty its button is disabled with "Set an endpoint in
  Settings". The app suggests no host.

### Stream `net`

Every 1 s while the throughput graph is on screen (the `live` flag of plan 001; owner, 2026-11-13), every 2 s for any other subscriber. Only the interface counters are read at 1 s; Wi-Fi details stay at their own slower interval.

```json
{"interfaces":[{"id":"wifi0","kind":"wifi","up":true,"default":true,
                "rxPerSec":0,"txPerSec":0,"rxBytes":0,"txBytes":0,
                "addrs":["192.0.2.10/24"],"speedMbps":null,
                "wifi":{"ssid":"…","signalPercent":70,"freqMhz":5180,"band":"5 GHz","rateMbps":866}}],
 "total":{"rxPerSec":0,"txPerSec":0},
 "gateway":"192.0.2.1","dns":["192.0.2.1"],"connectivity":"full",
 "latency":{"enabled":false,"target":null,"ms":null,"lossPercent":null},
 "today":{"rxBytes":0,"txBytes":0}}
```

Container and loopback interfaces are hidden by default and excluded from
`total`; a switch shows them. `total` counts physical and VPN interfaces
once: traffic through a VPN appears on both the tunnel and the physical
interface, so `total` uses only interfaces that hold a default route or
have no tunnel above them (the rule and its limits are a comment in
`total.rs` and a test).

Series: `net.<kind><n>.rx`, `net.<kind><n>.tx` for up to four interfaces
(bytes per minute), `net.latency`. The `daily` table gets bytes per
interface kind per day, kept 90 days.

### Tools (each started by a button, each cancellable)

| Tool | How |
|------|-----|
| Ping | Ten echoes to a host the user types; min, average, max, loss |
| Traceroute | Up to 30 hops; hop addresses are shown, never stored |
| DNS lookup | Name → addresses, with the resolver used |
| Speed test | Download for up to 10 s, upload for up to 10 s, with `curl --max-time`, to the configured endpoint; shows Mbps for each and the endpoint's name. **No endpoint ships: `net.speedtest.url` is an empty setting** (owner, 2026-11-13), and the Run button is disabled until the user sets one |
| Public address | One request to the service in `net.publicIp.url`, which is empty by default; Show is disabled until it is set; the card shows which service is asked |
| Latency monitor | Opt-in; one echo every 10 s while enabled **and** a consumer (the page, a pinned card or a bar item) is active |

### LAN devices (phase 2)

A "Devices on this network" card with a Refresh button: neighbour-table
entries on the default interface joined with mDNS names and service types,
vendor from the MAC prefix only if a small OUI table is shipped (decide in
step 1; default: no table, show the address). Not refreshed automatically.

### Traffic by app (phase 3)

A card listing the user's processes by bytes over the last interval,
grouped as in plan 006, from `sock_diag` dumps every 5 s while the card is
visible. Short-lived connections between two dumps are missed, and the
card says "TCP connections of your own apps, sampled".

### Page

1. Throughput: big down and up rates, a two-line graph with the range
   picker, today's totals.
2. Connection: the default interface's kind, address, gateway, DNS,
   connectivity; Wi-Fi name, signal, band and rate.
3. Interfaces: one row each, with a switch for the hidden kinds.
4. Latency (when enabled): a graph and loss.
5. Tools: a row of buttons, each opening a small result card.
6. Devices on this network; Traffic by app.
7. Usage: bytes per day for 30 and 90 days from `daily`, as bars.

Pinnable cards: `net.throughput`, `net.connection`. Bar item `net`.
CLI: `mirqab net [--json]`, `mirqab ping <host>`, `mirqab speedtest`.

## Rejected

- **A bundled speed-test server list or a third-party speed-test
  library.** Terms differ per provider; one configurable endpoint is
  honest.
- **A default public-IP request on opening the tab.** The address would
  leave the machine every time the tab opens.
- **`nmcli` parsing.** A process per refresh; the D-Bus properties and
  their change signals cost nothing between changes.
- **ARP sweeps or `nmap`.** Active scanning of a network the user may not
  own.
- **pcap, nethogs, eBPF.** Root.
- **Storing interface names in history.** A public bug report with the
  database attached would leak them.

## Tests and fixtures

Add to `root-desktop`: `proc/net/dev` (Ethernet, Wi-Fi, a bridge, a
container pair, a tunnel, loopback; names are generic) with a `.t1` twin,
`sys/class/net/*`, `proc/net/route`, `etc/resolv.conf`. Addresses only
from the documentation ranges (192.0.2.0/24, 198.51.100.0/24,
2001:db8::/32). A fake NetworkManager and Avahi on the private bus; stub
`ip`, `iw`, `resolvectl`, `curl`. The ICMP and traceroute sockets sit
behind a trait with a recorded double.

| Test | Checks |
|------|--------|
| `crates/mirqab-core/tests/net.rs` | Interface kinds; hidden kinds; rates from the delta; counter wrap and reset give no negative rate; the `total` rule with a tunnel |
| same | Wi-Fi from the fake NetworkManager and from stub `iw`; neither present gives `wifi: null` |
| same | **Privacy**: the minute writer's keys contain no interface name; no SSID or MAC in any `event` row; the public address is absent from every stream until requested and gone after `net.forget` |
| same | Latency off makes no echo; on, none without a consumer; one per 10 s with one |
| same | Speed test with no endpoint configured makes no request; `curl` argv is fixed and has `--proto =https` |
| `crates/mirqab-core/tests/netdiag.rs` | Ping statistics; traceroute hop parsing from the recorded double |
| `tests/test-ui-network.sh` (`mirqab-ui --selftest network`, plan 001) | Cards; the public address appears only after the button |
| `tools/secrets.sh` | Fixtures contain only documentation-range addresses |

## Acceptance

1. Rates match a large download measured by another tool within 5 %.
2. Wi-Fi details show on a Wi-Fi machine; an Ethernet-only machine shows
   none and no error.
3. With the tab closed and nothing pinned, the daemon makes no network
   request and no echo (check with the daemon's own `status` counters).
4. The privacy tests pass.

## Checklist

Phase 1:

1. Resolve Unverified.
2. `crates/mirqab-core/src/samplers/net/{mod,ifaces,total,route,dns,wifi,latency}.rs`;
   `dbus/networkmanager.rs`.
3. Series and `daily`; `tests/golden/net.json`; tests.
4. `crates/mirqab-core/src/netdiag/{ping,trace,lookup,speed,publicip}.rs`.
5. `crates/mirqab-view/src/tabs/network.rs` (the view model and the tab's cards); `crates/mirqab-ui/src/tabs/network/{mod,page,throughput_card,connection_card,interface_list,latency_card,tools_card,usage_card}.rs`.
6. CLI and bar item; Settings for endpoints and the latency switch.

Phase 2:

7. `samplers/net/lan.rs`, `dbus/avahi.rs`, `crates/mirqab-ui/src/tabs/network/lan_devices_card.rs`.

Phase 3:

8. `samplers/net/byapp.rs` (`sock_diag`), `crates/mirqab-ui/src/tabs/network/by_app_card.rs`, tests with a
   recorded netlink dump.

## Unverified

- NetworkManager D-Bus property names on 1.58 (`ActiveConnection`,
  `AccessPoint.Strength`, `Connectivity`): confirm at
  https://networkmanager.dev/docs/api/latest/ and with `busctl introspect`.
- Unprivileged UDP traceroute with `IP_RECVERR` returning the ICMP
  time-exceeded source: write a ten-line probe first.
- `tcpi_bytes_acked` and `tcpi_bytes_received` in `inet_diag` replies for
  another process's socket owned by the same uid.
- The reply shapes the two empty endpoint settings accept (a plain-text
  address or a JSON field for the public address; any HTTPS URL that
  serves and accepts bytes for the speed test): write them into the
  Settings help text. No endpoint is chosen or shipped (owner,
  2026-11-13), so no provider's terms need reading.
- Avahi's D-Bus browse API versus spawning `avahi-browse -ptr` once per
  Refresh (`avahi-browse` is present **[machine]**).
