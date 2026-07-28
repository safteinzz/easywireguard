# easywireguard (`ewg`)

WireGuard without the hand-editing: manage live interfaces wherever their
`.conf` files live, and generate full-mesh configs from one manifest. Pure Rust,
no `wg` binary required for keys or generation.

```bash
cargo install easywireguard
```

## Run interfaces (this box)

```bash
ewg                      # interactive TUI: list + toggle up/down
ewg list                 # every .conf you can bring up, with up/down state
ewg status               # only what's currently up
ewg up <name>            # bring one up   (found across your registered dirs)
ewg down <name>          # bring one down

ewg dir add <path>       # register where .conf files live (not just /etc/wireguard)
ewg dir                  # list registered dirs   (-v shows .conf counts)
ewg dir rm <path>
```

Reading `/etc/wireguard` needs root; `ewg` auto-elevates with `sudo` when a dir
isn't readable (set `EWG_NO_SUDO=1` to disable). Point at other dirs with
`--dir` or `$EWG_DIR`.

## Keys

```bash
ewg key                  # a private + public keypair (wg-compatible)
ewg psk                  # a preshared key
ewg pubkey <PRIVATE>     # derive a public key
```

## Mesh (design then generate)

Describe every node once, then generate each node's config as "all peers minus
itself".

```bash
ewg mesh add houseA --address 10.10.0.1/24 --pubkey <PUB> \
    --endpoint vpn-a:51820 --private <PRIV> -m mesh.toml
ewg mesh                 # list nodes   (-v verbose, --json machine-readable)
ewg mesh rm houseA
ewg mesh gen -o out/     # writes out/houseA.conf, ... (peers routed by /32)
```

`--private` is optional: omit it for a public-only manifest and paste keys in
later. `mesh list --json` never includes private keys.

### Full-tunnel exits & site-to-site

By default each node advertises just its own `/32`. Override what peers route to a
node with `--allowed-ips` — `0.0.0.0/0` makes it a **full-tunnel exit**, or pass a
LAN subnet for **site-to-site**. `--dns` sets a node's own resolver, `--keepalive`
holds the tunnel open through NAT:

```bash
# a home router that roamers exit through, with a Pi-hole behind it
ewg mesh add home --address 10.10.1.1/24 --pubkey <PUB> \
    --endpoint vpn.example.com:51820 --allowed-ips 0.0.0.0/0 -m mesh.toml
# a phone that full-tunnels home and uses the Pi-hole for DNS
ewg mesh add phone --address 10.10.1.2/24 --pubkey <PUB> \
    --dns 192.168.10.250 --keepalive 25 --private <PRIV> -m mesh.toml
```

## QR (onboard a phone)

Render any WireGuard config as a scannable QR — a `.conf` file, or a node
generated straight from the manifest — then scan it in the phone's WireGuard app.

```bash
ewg qr out/phone.conf        # QR for a file
ewg qr phone -m mesh.toml    # QR for that node's generated config
ewg qr phone -o phone.png    # also write a PNG to save/share
```

Prints a compact QR to the terminal (scan it off the screen). If the config has
no private key, `ewg` warns you — that QR wouldn't import a working tunnel.
