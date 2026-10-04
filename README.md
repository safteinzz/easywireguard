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

## Commands

The TUI wraps these; call them directly to automate.

```bash
ewg list                 # every interface across your dirs, up or down
ewg status               # only the ones up; --json for a status bar, no root
ewg up <name>            # bring one up
ewg down <name>          # and take it down
ewg dir add <path>       # register where .conf files live, not just /etc/wireguard
ewg check <path>...      # validate configs, non-zero exit if one is broken
ewg key                  # a new keypair
ewg psk                  # a new preshared key
ewg pubkey <private>     # the public half of a private key
ewg mesh add <name> ...  # a node: --address and --pubkey, --endpoint for a hub
ewg mesh gen -o out/     # write every node's .conf
ewg qr <node>            # that node's config as a QR, for the phone app
```

Every command takes `--dir` to use one directory for that run, and
`ewg <command> --help` has the rest, `mesh add`'s hub, exit and DNS flags
included.

## Keys

| key | does |
| --- | --- |
| `j` `k` / `↑` `↓` | move in the list |
| `h` `l` / `←` `→` / `Tab` | switch tab |
| `q` / `Esc` / `Ctrl-C` | quit |

Each tab's own keys are on its bottom line.

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

## Compatibility

Linux. Interfaces are brought up through `wg-quick` and `systemctl`, so the
interface side needs a systemd machine; key generation and mesh config are pure
Rust and work anywhere.

## License

AGPL-3.0-only
