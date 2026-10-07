# easywireguard (`ewg`)

> **Canonical:** [gitlab.com/safteinzz/easywireguard](https://gitlab.com/safteinzz/easywireguard) · **Mirror:** [github.com/safteinzz/easywireguard](https://github.com/safteinzz/easywireguard)

<!-- desc:start -->
mesh, minus the mess - interfaces, keys and full-mesh configs in one CLI + TUI
<!-- desc:end -->

## Install

```bash
cargo install easywireguard
ewg self check   # is a newer release out?
ewg self update  # install the latest
```

No cargo yet? Rust installs the same way on every distro: [rustup.rs](https://rustup.rs).

## Manage every interface

![The interface manager listing five WireGuard configs, two of them up and two flagged to start at boot, with the action keys along the bottom](https://gitlab.com/safteinzz/easywireguard/-/raw/main/readme-assets/interfaces.png)

Legend: `● up` running · `○ down` stopped · `⏻ boot` starts with the machine

```bash
ewg                      # the interface manager
ewg list                 # every interface across your dirs, up or down
ewg up <name>            # bring one up
ewg down <name>          # and take it down
```

Every `.conf` across the dirs you registered, not just `/etc/wireguard`, with what it is doing right now.

## Tell whether it is actually working

![The inspector over the interface list, showing a config followed by a live wg show readout with a handshake a minute old and transfer counters](https://gitlab.com/safteinzz/easywireguard/-/raw/main/readme-assets/inspect.png)

```bash
ewg status               # only the ones up; --json for a status bar, no root
```

`i` shows the config, and for a running interface a live `wg show` under it: the peer, the last handshake, the bytes moved.

## See the whole mesh

![The Mesh tab showing two hubs with their spokes nested underneath, each row with its mesh IP and endpoint](https://gitlab.com/safteinzz/easywireguard/-/raw/main/readme-assets/mesh.png)

```bash
ewg mesh add <name> ...  # a node: --address and --pubkey, --endpoint for a hub
ewg mesh gen -o out/     # write every node's .conf
```

Describe each node once and it is laid out hub and spoke. A hub meshes with every other hub and a spoke lists only its hub, so phones never get useless peer blocks for each other.

## Create a node without touching a key

![The node wizard with a Spoke/Hub toggle and fields for name, address, DNS, the hub to dial, and where the private key goes](https://gitlab.com/safteinzz/easywireguard/-/raw/main/readme-assets/wizard.png)

`c` generates the keypair for you. **store** keeps the private key in the manifest so you can export a working config later, and **redact** leaves it only in what you hand out, so nothing secret is at rest.

## Onboard a phone by scanning

![A scannable QR code of a node's generated config, over the mesh list, titled scan phone](https://gitlab.com/safteinzz/easywireguard/-/raw/main/readme-assets/qr.png)

```bash
ewg qr <node>            # that node's config as a QR, for the phone app
```

`↵` on a node shows its config as a QR that scans on any terminal theme, so a phone needs no file transfer. `E` exports the same config as a file, an install to `/etc/wireguard`, a PNG or an Ansible peer entry.

## Commands

```bash
ewg dir add <path>       # register where .conf files live
ewg check <path>...      # validate configs, non-zero exit if one is broken
ewg key                  # a new keypair
ewg psk                  # a new preshared key
ewg pubkey <private>     # the public half of a private key
```

Every command takes `--dir` to use one directory for that run, `ewg <command> --help` has the details, and `?` in the TUI lists every key.

Output is for people, and `dir`, `mesh` and `status` take `--json` with fixed field names. `check` exits non-zero when a config is broken, and every failure names itself on stderr and exits non-zero.

## Where it keeps things

```
/etc/wireguard/*.conf     your interfaces, beside every dir you register
~/.config/ewg/dirs.toml   the dirs you registered
./mesh.toml               the mesh, read from the folder you run ewg in
./out/                    the configs g and mesh gen write
```

`mesh.toml` may hold private keys for `store` nodes, so gitignore it unless every node is redacted.

## Notes

- Reading `/etc/wireguard` needs root, so `ewg` reruns itself with `sudo`; `EWG_NO_SUDO=1` stops that, and `status --json` never needs it.
- A node advertises just its own `/32`. Set `allowed-ips` to `0.0.0.0/0` to make a hub a full-tunnel exit, or to a LAN subnet for site-to-site.
- Start-on-boot uses systemd (`wg-quick@<name>`), and toggling an interface runs your `wg-quick`.

## Compatibility

Linux. Interfaces are brought up through `wg-quick` and `systemctl`, so the
interface side needs a systemd machine; key generation and mesh config are pure
Rust and work anywhere.

## License

AGPL-3.0-only
