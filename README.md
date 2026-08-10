# easywireguard (`ewg`)

> **Canonical:** [gitlab.com/safteinzz/easywireguard](https://gitlab.com/safteinzz/easywireguard) · **Mirror:** [github.com/safteinzz/easywireguard](https://github.com/safteinzz/easywireguard)

**Mesh, minus the mess. 🕸️**

A tabbed TUI for humans and a flag-driven CLI for scripts, in one small binary.
Manage live interfaces, describe a whole hub-and-spoke or full mesh in a single
manifest, and onboard a phone by scanning a QR. Pure Rust, so no `wg` binary is
needed for keys or config generation.

```bash
cargo install easywireguard   # the command you type is the shorter `ewg`
```

## Manage every interface

Legend: `● up` running · `○ down` stopped · `⏻ boot` starts on boot

Bare `ewg` opens on the interface manager: every `.conf` across the dirs you
registered, with its live state. `↵` toggles one up or down, `c` creates one in
your `$EDITOR` (paste a provider config and it is validated before it lands), `e`
edits, `d` deletes it keeping a `.bak`, `b` flips start-on-boot, and `i` inspects
it with a live `wg show` when it is up.

![The interface manager listing four WireGuard configs with their up/down state, and the action keys along the bottom](https://gitlab.com/safteinzz/easywireguard/-/raw/main/readme-assets/interfaces.png)

## See the whole mesh

Describe each node once and `ewg` lays them out hub-and-spoke, spokes nested under
the hub they dial. A hub has an endpoint and meshes with every other hub; a spoke
has none and lists only its hub, so phones never get useless peer blocks for each
other. Each node's `.conf` is then generated as exactly the peers it should reach.

![The Mesh tab showing two hubs with two spokes nested under one of them, each row with its mesh IP and endpoint](https://gitlab.com/safteinzz/easywireguard/-/raw/main/readme-assets/mesh.png)

By default a node advertises just its own `/32`. Set `allowed-ips` to `0.0.0.0/0`
to make a hub a full-tunnel exit, or to a LAN subnet for site-to-site.

## Create a node without touching a key

`c` opens a wizard (or `e` to edit an existing node): pick **Spoke** or **Hub**,
fill a couple of fields, and `ewg` generates the keypair for you. Choose **store**
to keep the private key in the manifest so you can re-export a working config
later, or **redact** so it lives only in the QR and file handed out at create -
nothing secret left at rest.

![The node wizard with a Spoke/Hub type toggle and fields for name, address, DNS and the hub to dial](https://gitlab.com/safteinzz/easywireguard/-/raw/main/readme-assets/wizard.png)

## Onboard a phone by scanning

`↵` on a node renders its config as a real black-on-white QR that scans on any
terminal theme and shrinks to fit the window. Open the WireGuard app, scan, done -
no file transfer. `E` exports the same config as `out/<name>.conf`, an install to
`/etc/wireguard`, a PNG, or an Ansible peer entry.

![A scannable QR code of a node's generated config filling the screen, titled scan phone](https://gitlab.com/safteinzz/easywireguard/-/raw/main/readme-assets/qr.png)

## CLI

The TUI wraps these; call them directly to automate.

```bash
# interfaces
ewg list | status | up <name> | down <name>
ewg dir add <path>                 # register where .conf files live (not just /etc/wireguard)

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
- Start-on-boot uses systemd (`wg-quick@<name>`); toggling interfaces shells out
  to your `wg-quick`. Keys and config generation are pure Rust, never a wrapper
  around `wg`.

## License

AGPL-3.0-only.
