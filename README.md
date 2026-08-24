# easywireguard (`ewg`)

> **Canonical:** [gitlab.com/safteinzz/easywireguard](https://gitlab.com/safteinzz/easywireguard) · **Mirror:** [github.com/safteinzz/easywireguard](https://github.com/safteinzz/easywireguard)

<!-- desc:start -->
mesh, minus the mess - interfaces, keys and full-mesh configs in one CLI + TUI
<!-- desc:end -->

## Install

```bash
cargo install easywireguard   # the command you type is the shorter `ewg`
ewg self update               # reinstall the latest release later on
```

![A tour of ewg: inspecting a running interface, toggling one up and down, browsing the mesh, creating a node with a generated keypair, exporting it as an Ansible entry, deleting it, and generating every node's config](https://gitlab.com/safteinzz/easywireguard/-/raw/main/readme-assets/demo.gif)

## Manage every interface

Bare `ewg` opens on the interface manager: every `.conf` across the dirs you
registered, with what it is doing right now. `● up` is running, `○ down` is
stopped, `⏻ boot` starts with the machine.

![The interface manager listing five WireGuard configs, two of them up and two flagged to start at boot, with the action keys along the bottom](https://gitlab.com/safteinzz/easywireguard/-/raw/main/readme-assets/interfaces.png)

## Tell whether it is actually working

`i` shows the config, and for a running interface a live `wg show` under it:
the peer, the last handshake, the bytes moved.

![The inspector over the interface list, showing a config followed by a live wg show readout with a handshake a minute old and transfer counters](https://gitlab.com/safteinzz/easywireguard/-/raw/main/readme-assets/inspect.png)

## See the whole mesh

Describe each node once and `ewg` lays them out hub-and-spoke, spokes nested
under the hub they dial. A hub has an endpoint and meshes with every other hub;
a spoke has none and lists only its hub, so phones never get useless peer blocks
for each other.

![The Mesh tab showing two hubs with their spokes nested underneath, each row with its mesh IP and endpoint](https://gitlab.com/safteinzz/easywireguard/-/raw/main/readme-assets/mesh.png)

By default a node advertises just its own `/32`. Set `allowed-ips` to `0.0.0.0/0`
to make a hub a full-tunnel exit, or to a LAN subnet for site-to-site.

## Create a node without touching a key

`c` opens a wizard: pick **Spoke** or **Hub**, fill a couple of fields, and the
keypair is generated for you. **store** keeps the private key in the manifest so
you can re-export a working config later; **redact** leaves it only in the QR and
file handed out at create, nothing secret at rest.

![The node wizard with a Spoke/Hub toggle and fields for name, address, DNS, the hub to dial, and where the private key goes](https://gitlab.com/safteinzz/easywireguard/-/raw/main/readme-assets/wizard.png)

## Onboard a phone by scanning

`↵` on a node renders its config as a black-on-white QR that scans on any
terminal theme. Open the WireGuard app, scan, done - no file transfer. `E`
exports the same config as `out/<name>.conf`, an install to `/etc/wireguard`, a
PNG, or an Ansible peer entry.

![A scannable QR code of a node's generated config, over the mesh list, titled scan phone](https://gitlab.com/safteinzz/easywireguard/-/raw/main/readme-assets/qr.png)

## Keys

| key | Interfaces | Mesh |
| --- | --- | --- |
| `j/k` `↑↓` | move | move |
| `h/l` `←→` `tab` | switch tab | switch tab |
| `↵` | toggle up / down | show the QR |
| `c` | new config in `$EDITOR` | create a node |
| `e` | edit it in `$EDITOR` | edit the node |
| `d` | delete (keeps a `.bak`) | remove from the manifest |
| `i` | inspect, with live `wg show` | view the generated config |
| `b` | toggle start-on-boot | |
| `R` | | rotate the keypair |
| `E` | | export (file, install, QR, Ansible) |
| `g` | | generate every node's config |
| `r` | refresh | reload the manifest |
| `q` `esc` | quit | quit |

A config pasted into `$EDITOR` is validated before it lands, so a truncated
paste is caught there rather than at `wg-quick up`.

## CLI

The TUI wraps these; call them directly to automate.

```bash
# interfaces
ewg list | status | up <name> | down <name>
ewg dir add <path>                 # register where .conf files live (not just /etc/wireguard)
ewg check /etc/wireguard/*.conf    # validate configs, non-zero exit if any is broken

# keys
ewg key | psk | pubkey <PRIVATE>

# mesh: design, then generate each node's config
ewg mesh add flint --address 10.10.1.1/24 --pubkey <PUB> \
    --endpoint vpn.example.com:51820 --allowed-ips 0.0.0.0/0     # a hub / exit
ewg mesh add phone --address 10.10.1.3/24 --pubkey <PUB> \
    --hub flint --dns 192.168.10.250 --keepalive 25 [--private <PRIV>]
ewg mesh                           # list (-v verbose, --json; never prints private keys)
ewg mesh rm <name>
ewg mesh gen -o out/               # write every node's .conf

# QR (onboard a phone)
ewg qr <node> -m mesh.toml         # QR of that node's generated config
ewg qr out/phone.conf -o phone.png # a .conf file, also written as a PNG

# the tool itself
ewg self check | self update
```

`--hub` (repeatable) makes a node a spoke of those hubs; omit it and a spoke
reaches all hubs. `--private` is optional: omit it for a public-only manifest and
inject keys later.

## Notes

- Reading `/etc/wireguard` needs root; `ewg` auto-elevates with `sudo` (disable
  with `EWG_NO_SUDO=1`). Point elsewhere with `--dir` or `$EWG_DIR`.
- `mesh.toml` may hold private keys for `store`-mode nodes, so treat it as a
  secret and gitignore it; a redacted or public-only manifest is safe to commit.
  `mesh list --json` never prints private keys.
- The Mesh tab reads `mesh.toml` from the directory you run `ewg` in, and `g`
  writes to `./out` beside it.
- Start-on-boot uses systemd (`wg-quick@<name>`); toggling interfaces shells out
  to your `wg-quick`. Keys and config generation are pure Rust, never a wrapper
  around `wg`.

## License

AGPL-3.0-only.
