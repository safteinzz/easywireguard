# easywireguard (`ewg`)

WireGuard without the hand-editing. A tabbed TUI for humans and a flag-driven CLI
for scripts/agents: manage live interfaces, generate **hub-and-spoke** or full-mesh
configs from one manifest, and onboard devices by QR. Pure Rust — no `wg` binary
needed for keys or generation.

```bash
cargo install easywireguard   # the command you type is the shorter `ewg`
```

## The TUI (just run `ewg`)

Bare `ewg` opens a two-tab toolbox — `h/l ←→` switch tabs, `j/k ↑↓` move:

- **Interfaces** — every `.conf` across your registered dirs; `↵`/`u`/`d` bring them
  up/down, `i` inspects one.
- **Mesh** — the nodes in a manifest, shown **hub-and-spoke** (spokes nested under
  their hub). Per node: `c` create, `e` edit, `R` rotate keys, `d` delete, `E` export,
  `↵` show its QR, `i` view its generated `.conf`.

The **create wizard** (`c`) is two toggles + a few fields (`←/→` flip a toggle,
`ctrl-hjkl`/Tab move between fields):

- **Type: Spoke │ Hub** — a *spoke* is a road-warrior that dials the hub(s) you pick;
  a *hub* is reachable (has an endpoint) and meshes with every other hub.
- **Key: Generate │ Paste** — *generate* a fresh keypair, or *paste* an existing public
  key (e.g. a router's, whose private lives elsewhere).
  - On **generate** you also choose **store** (keep the private key in the manifest so you
    can re-export a working config later) or **redact** (private only in the QR/file at
    create — the phone case, nothing secret at rest).

**Export** (`E`) offers: write `out/<name>.conf`, install to `/etc/wireguard`, show the
QR, or print the peer entry for an Ansible `wg_peers` list. The QR renders as real
black-on-white cells (scans on any terminal theme) and shrinks to fit. In any text box,
`y` copies to the clipboard (wl-copy/xclip/pbcopy, or an OSC 52 escape over SSH).

## The mesh model

Describe every node once; each node's config is generated as the peers it should reach:

- a **hub** (has an `endpoint`) meshes with every other hub, plus the spokes that point at it;
- a **spoke** (no endpoint) lists only its hub(s) — so phones don't get useless peer
  blocks for each other.

By default a node advertises just its own `/32`. Override with `allowed-ips`: `0.0.0.0/0`
makes a hub a **full-tunnel exit**, a LAN subnet makes it **site-to-site**.

## CLI (scripting / agents)

The TUI wraps these; use them directly to automate.

```bash
# interfaces (this box)
ewg list | status | up <name> | down <name>
ewg dir add <path>            # register where .conf files live (not just /etc/wireguard)

# keys
ewg key | psk | pubkey <PRIVATE>

# mesh: design, then generate "each node's config"
ewg mesh add phone --address 10.10.1.2/24 --pubkey <PUB> --hub flint \
    --dns 192.168.10.250 --keepalive 25 [--private <PRIV>]
ewg mesh add flint --address 10.10.1.1/24 --pubkey <PUB> \
    --endpoint vpn.example.com:51820 --allowed-ips 0.0.0.0/0    # a hub / exit
ewg mesh            # list (-v verbose, --json machine-readable; never prints private keys)
ewg mesh rm <name>
ewg mesh gen -o out/                                            # write every node's .conf

# QR (onboard a phone)
ewg qr <name> -m mesh.toml        # QR of that node's generated config
ewg qr out/phone.conf -o phone.png
```

`--hub` (repeatable) makes a node a spoke of those hubs; omit it and a spoke reaches all
hubs. `--private` is optional — omit it for a public-only manifest and inject keys later.

## Notes

- Reading `/etc/wireguard` needs root; `ewg` auto-elevates with `sudo` (disable with
  `EWG_NO_SUDO=1`). Point elsewhere with `--dir` or `$EWG_DIR`.
- The manifest (`mesh.toml`) may hold private keys for `store`-mode nodes — treat it as a
  secret (gitignore it); a redacted or public-only manifest is safe to commit.
