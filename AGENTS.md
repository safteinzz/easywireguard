<!--
AI-ONLY DOCUMENT. This file exists to give an AI agent the COMPLETE operating picture for this repo. Optimize for completeness and precision for the agent, not for human readability. Humans read README.md instead. Do not remove detail to make this nicer, err toward more explicit, not less. FORMAT: machine-read, not a formatted human doc. Do NOT hard-wrap lines to a column width for readability; put each rule/point on ONE line, however long.
-->
# AGENTS.md

Working brief for an AI coding agent, not documentation for people (the README covers that): the rules, invariants and gotchas needed to change this project correctly without rediscovering them.

## Hard rules
- Test by driving the built binary against temp dirs only; never touch real WireGuard interfaces, `/etc/wireguard`, or a user's real keys.

## Invariants and gotchas
- Keys are pure Rust (x25519-dalek); no `wg` binary is required. The private key is clamped on generation so output round-trips identically to `wg genkey`/`wg pubkey`; when touching key code, keep the clamp or generated keys diverge from what WireGuard accepts. A regression test pins a known private/public pair; keep it.
- Public key is `x25519(private, basepoint)`; that function clamps its scalar, matching `wg pubkey`.
- Mesh generation is "all peers minus self": a node must never appear in its own `[Peer]` list, since its address is already the `[Interface]` and a self-peer collides on AllowedIPs.
- Peers are routed by `/32` (the node's single mesh IP), never the interface prefix, or traffic for the whole subnet would be sent to one peer.
- `private_key` is optional in the manifest so a public-only manifest can be shared; a missing key writes a `<PASTE PRIVATE KEY>` placeholder, never a crash or a silently-wrong config. `mesh list --json` never includes private keys.
- The manifest rejects duplicate node names and duplicate mesh addresses; overlapping addresses break routing.
- Command taxonomy is deliberate: ops (`list`/`status`/`up`/`down`/`dir`/TUI) act on live `.conf` files across registered dirs; mesh (`mesh add`/`list`/`rm`/`gen`) edits a manifest and generates configs. `status` shows only interfaces that are up; `list` (alias `ls`) shows all with up/down state. Keep these separate.
- Reading the config dir and toggling interfaces needs root; the tool auto-re-execs under `sudo` (via its own absolute path) when a dir is unreadable. `EWG_NO_SUDO=1` disables that; `EWG_REGISTRY` and `EWG_DIR` override paths (tests rely on both).
- `wg-quick` output is captured, not inherited, or its wall of `[#] ...` lines corrupts the TUI's alternate screen.
- Color is TTY-gated in `main` (disabled when stdout is not a terminal) so piped output stays byte-for-byte clean.
- Which wizard fields carry the red `*`, matching what the submit path refuses: a node's Name always, a hub's Endpoint (the field only exists in the Hub shape, so the star is never shown where it would not apply), the pasted Public key in the create wizard's Paste shape (the edit wizard has no key fields at all, since an edit never re-keys a node), and the interface Name, which `wg::valid_iface_name` rejects when blank. The Hub-to-dial and Private-key picks are not starred: a spoke with no hub yet is a node you finish later.

## Build / lint / test
- `cargo build --release`, binary at `target/release/ewg`.
- Unit tests sit at the bottom of the source file they cover, end-to-end tests in `tests/cli.rs`; drive the built binary against temp dirs only, never real WireGuard interfaces, `/etc/wireguard`, or real keys.

## README assets
- Every screenshot and the demo GIF are rendered by [VHS](https://github.com/charmbracelet/vhs) from the committed rig in `demo/`, never captured by hand: `demo/stage.sh` builds the world, `shots.tape` renders the list-shaped stills, `overlays.tape` the two that need a taller terminal (the QR, the inspector), `demo.tape` the tour. Run one tape at a time (`cd demo && vhs shots.tape`) - they share the stage and would tear each other's fixtures down mid-take.
- `demo/stage.sh` stands `wg`, `wg-quick` and `systemctl` up as shims on the staged `PATH` so an interface can be toggled on camera. The tool is unmodified; only the world under it is invented, which is what keeps the rig inside the rule about never touching real interfaces. The stage lives under `$TMPDIR`, outside this working tree, so nothing in frame can pick up this repo's branch and dirty count.
- A leak here means a real config in a frame - an endpoint, the DNS behind a tunnel, a public IP, a key - and since crates.io and every mirror fetch README images live, one rendered is one published: the fix is yanking the release and rotating what was in the shot. The rig enforces against that rather than hoping: everything staged runs under `env -i` with the allowlist in `env_for_stage`, because an exported `EWG_DIR` beats the staged registry and would point every screenshot at the renderer's real `/etc/wireguard`. A username or a clock in frame is not that kind of leak; the demo shell replaces the real `~/.bashrc` for reproducibility, and here for a second reason too - this machine's rc runs `fastfetch`, which paints host, distro, kernel and uptime across the shot one missing `clear` from the GIF.
- The rig must not be able to damage the machine: it writes only inside the stage, runs nothing privileged, its `systemctl` stand-in shadows the real one on the staged `PATH` only, and `down` deletes solely a tree carrying the `.ewg-demo-stage` marker `up` wrote - so an `EWG_DEMO_HOME` pointed at a real directory, at `$HOME` or at a system dir gets a refusal, symlinks resolved first and `--one-file-system` behind that. Test the refusals after touching them, never just the happy path.
- Nothing in a frame is real: RFC 5737 / RFC 3849 addresses, `example.com` names, keys generated by the build under test and thrown away with the stage. Nothing about the rig belongs in the README - a reader installing from crates.io got a package with `demo/` excluded and could not run it anyway.

## Overview
Layout:
- `src/main.rs` - the clap `Cmd` enum and the dispatch match, nothing else.
- `src/commands/<verb>.rs` - one file per command, each exposing `run`, with a command's own argument types (`DirArgs`, `MeshArgs`) beside it.
- `src/tui/` - the toolbox: `mod.rs` owns `App` and the event loop, `input.rs` dispatches keys, `interfaces.rs` and `mesh.rs` hold each tab's actions, `wizard.rs` is what a submitted prompt writes, `prompt.rs` is the wizard's field machinery, `render.rs` draws the frame, `overlay.rs` the centered windows, `widgets.rs` the domain-blind furniture, `edit.rs` the `$EDITOR` handoff, `clipboard.rs` the OSC 52 copy.
- Domain modules at the top level: `wg` (interfaces and `wg-quick`), `manifest` (the mesh file), `keys` (x25519), `registry` (where configs live), `elevate` (re-exec under sudo), `selfcmd` (`ewg self`).


`ewg` (crate `easywireguard`) generates WireGuard key material and manages live interfaces, and from a single mesh manifest generates each node's `.conf` as "all peers minus itself". Built for full-mesh WireGuard (every node an equal peer, no central hub) plus day-to-day interface management, with no external `wg` binary needed for its core.
If this file contradicts the code, the code wins; fix this file the same session.

## Self-repair
If anything here contradicts the code, the code wins; fix AGENTS.md in the same session you notice the drift.
