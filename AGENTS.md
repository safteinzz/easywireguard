<!-- For AI agents, not people: README is for people. Every invariant and gotcha needed to change this repo, each with its one reason, telegraphic. Never how a fact was found or what changed. One point per line, never hard-wrapped. -->
# AGENTS.md

## Hard rules
- Test by driving built binary against temp dirs only; never touch real WireGuard interfaces, `/etc/wireguard`, or user's real keys.

## Invariants and gotchas
- Keys pure Rust (x25519-dalek); no `wg` binary required. Private key clamped on generation so output round-trips identically to `wg genkey`/`wg pubkey`; keep clamp or generated keys diverge from what WireGuard accepts. Regression test pins known private/public pair; keep it.
- Hub = node with `endpoint`, spoke = without. Hub peers with every hub and every spoke naming it or naming none; spoke peers only its `hubs` (every hub when empty). Node never in own `[Peer]` list, since its address is already `[Interface]` and self-peer collides on AllowedIPs.
- Peer's AllowedIPs defaults to its `/32` (single mesh IP), never interface prefix, else whole subnet's traffic goes to one peer; node's `allowed_ips` overrides it (full-tunnel exit, LAN subnet).
- `private_key` optional in manifest so public-only manifest can be shared; missing key writes `<PASTE PRIVATE KEY>` placeholder, never crash or silently wrong config. `mesh list --json` never includes private keys.
- Manifest rejects duplicate node names and duplicate mesh addresses; overlapping addresses break routing.
- Every entry in node's `dns` must be IP or search domain: `check_dns` in `Manifest::add` (TUI create and edit both go through it) and on `Manifest::load`. Android app splits `DNS` on commas, takes what parses as IP as server, what matches its hostname pattern as search domain, refuses to import config for anything else (`host:port`, empty entry before comma), so `check_dns` refuses every empty entry. wg-quick takes entry with colon, or only digits and dots, as server, all else as search domain, so `is_search_domain` also refuses name of only digits and dots (mistyped IP). Search domain alone accepted; wg-quick then sets no DNS.
- Only hub's generated config carries `ListenPort` (manifest's `listen_port`); spoke gets none, so takes free port, never clashes with another VPN holding 51820 on same device.
- Create wizard: spoke's Hub to dial sits above Address, because Address defaults to next free IP in picked hub's IPv4 subnet, re-applied by `refresh_address_default` after every choice change; no usable hub subnet falls back to `10.99.0.0/24`. Edit wizard keeps node's own address.
- Command taxonomy deliberate, keep separate: ops (`list`/`status`/`up`/`down`/`dir`/TUI) act on live `.conf` files across registered dirs; mesh (`mesh add`/`list`/`rm`/`gen`) edits manifest and generates configs.
- Reading config dir and toggling interfaces needs root; tool auto re-execs under `sudo` (via own absolute path) when dir unreadable. Exception `status --json`: reads kernel (`/sys/class/net`, `DEVTYPE=wireguard`), never elevates, so status bar can poll it. `EWG_NO_SUDO=1` disables re-exec; `EWG_REGISTRY` and `EWG_DIR` override paths (tests rely on both).
- Child output captured, never inherited (`wg-quick`, `wg`, `systemctl`), else lines like wg-quick's `[#] ...` corrupt TUI's alternate screen.
- Red `*` fields match what submit path refuses: node's Name always; hub's Endpoint (field exists only in Hub shape, so star never shown where it does not apply); pasted Public key in create wizard's Paste shape (edit wizard has no key fields, since edit never re-keys node); interface Name, which `wg::valid_iface_name` rejects when blank. Hub-to-dial and Private-key picks not starred: spoke with no hub yet is node you finish later.

## Build / lint / test
- `cargo build --release`, binary at `target/release/ewg`.
- Unit tests at bottom of source file they cover; end-to-end tests in `tests/cli.rs`.

## README assets
- Every screenshot rendered by [VHS](https://github.com/charmbracelet/vhs) from committed rig in `demo/`, never captured by hand: `demo/stage.sh` builds world, `shots.tape` renders 15-row stills (interfaces, mesh, wizard), `overlays.tape` the two needing taller terminal (QR, inspector).
- `demo/render.sh` renders every tape (or named ones) in container, so machine needs only podman or docker. Image is `demo/Dockerfile`: vhs's own image pinned by digest, apt pointed at `snapshot.debian.org` at date in its `SNAPSHOT` arg (nothing installed on top yet, since stage and shims are bash and coreutils).
- Each tape runs in own container (stage, record, tear down), because tapes share stage and would tear down each other's fixtures mid-take; as your user (`--userns=keep-id`, or docker's `--user`) so images stay yours and prompt ends in `$`; no network.
- `demo/stage.sh` puts `wg`, `wg-quick` and `systemctl` shims on staged `PATH` so interface can be toggled on camera. Tool unmodified, only world under it invented: that keeps rig inside rule about never touching real interfaces. Stage lives under `$TMPDIR`, outside this working tree, so nothing in frame picks up repo's branch and dirty count.
- Leak means real config in frame (endpoint, DNS behind tunnel, public IP, key); crates.io and every mirror fetch README images live, so one rendered is one published, and fix is yanking release and rotating what was in shot. Rig enforces against it: everything staged runs under `env -i` with allowlist in `env_for_stage`, because exported `EWG_DIR` beats staged registry and would point every screenshot at renderer's real `/etc/wireguard`.
- Username or clock in frame is not that kind of leak. Demo shell replaces real `~/.bashrc`, for reproducibility and because a real rc can paint host, distro and uptime into frame.
- Rig must not damage machine: writes only inside stage, runs nothing privileged, `systemctl` stand-in shadows real one on staged `PATH` only, and `down` deletes only tree carrying `.ewg-demo-stage` marker `up` wrote, so `EWG_DEMO_HOME` pointed at real directory, `$HOME` or system dir gets refusal; symlinks resolved first, `--one-file-system` behind that. Test refusals after touching them, never just happy path.
- Nothing in frame real: RFC 5737 / RFC 3849 addresses, `example.com` names, keys generated by build under test and thrown away with stage.
- Nothing about rig in README, because reader installing from crates.io got package with `demo/` excluded and could not run it anyway.

## Overview
Layout:
- `src/main.rs`: clap `Cmd` enum and dispatch match, nothing else.
- `src/commands/<verb>.rs`: one file per command, each exposing `run`, command's own argument types (`DirArgs`, `MeshArgs`) beside it.
- `src/tui/` is toolbox: `mod.rs` owns `App` and event loop, `input.rs` dispatches keys, `interfaces.rs` and `mesh.rs` hold each tab's actions, `wizard.rs` what submitted prompt writes, `prompt.rs` wizard's field machinery, `render.rs` draws frame and `?` help panel, `overlay.rs` centered windows (form, QR and inspector readers, gate, typed gate before deleting node, export picker, offer to correct invalid config), `widgets.rs` domain-blind furniture and words every footer and key row is built from, `filter.rs` what `/` keeps (every selection indexes filtered rows, through `iface_rows` and `mesh_rows`), `line_edit.rs` one line of typed text with cursor and shell's editing keys, used by every form field, `/` query and typed gate, `edit.rs` `$EDITOR` handoff, `clipboard.rs` copy (system tool, else OSC 52).
- Top-level domain modules: `wg` (interfaces and `wg-quick`), `manifest` (mesh file), `keys` (x25519), `registry` (where configs live), `elevate` (re-exec under sudo), `selfcmd` (`ewg self`).

`ewg` (crate `easywireguard`) generates WireGuard key material, manages live interfaces, and from single mesh manifest generates each node's `.conf` (hubs full-meshed, spokes dialing their hubs). Built for mesh plus day-to-day interface management; keys and mesh need no external `wg` binary.

## Self-repair
If anything here contradicts code, code wins; fix AGENTS.md in same session you notice drift.
