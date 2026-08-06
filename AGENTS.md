<!--
AI-ONLY DOCUMENT. This file exists to give an AI agent the COMPLETE operating picture for this repo. Optimize for completeness and precision for the agent, not for human readability. Humans read README.md instead. Do not remove detail to make this nicer — err toward more explicit, not less. FORMAT: machine-read, not a formatted human doc. Do NOT hard-wrap lines to a column width for readability; put each rule/point on ONE line, however long.
-->
# AGENTS.md

## Hard rules
- Never commit during development. Commit only when the user says ship, then in this exact order: `cargo clippy` warning-clean and `cargo test` all green, bump `version` in Cargo.toml, one commit, `git push origin main`, `cargo publish` (dry-run first), and tag `vX.Y.Z` ONLY after publish succeeds (a tag must never point at a version that failed to publish).
- Commit messages: short conventional one-liners (`feat:`, `fix:`, `release vX.Y.Z:`). Never co-authored, no trailers.
- No em-dashes in any user-facing text (help, errors, README, commit messages).
- Test by driving the built binary against temp dirs only; never touch real WireGuard interfaces, `/etc/wireguard`, or a user's real keys.
- A test exists to catch a FUTURE silent regression (key/base64 correctness, config generation, address allocation, parsing) - never write one just to prove the edit you made landed (keybinding wiring, a wizard prefilling, a renamed field, an output string); delete those on sight.
- TUI/UI changes are verified by the USER: make the change, say what to look at, and ask - no `TestBackend` render dumps and no greps to confirm your own edit applied.

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
- User-facing errors are lowercase, name things in backticks, and say what to do; never let a raw OS error reach the user.

## Build / test
- `cargo build` / `cargo build --release`
- `cargo test` (unit tests at the bottom of each source file, e2e in `tests/cli.rs`)
- `cargo clippy`

## Overview
`ewg` (crate `easywireguard`) generates WireGuard key material and manages live interfaces, and from a single mesh manifest generates each node's `.conf` as "all peers minus itself". Built for full-mesh WireGuard (every node an equal peer, no central hub) plus day-to-day interface management, with no external `wg` binary needed for its core.

If this file contradicts the code, the code wins; fix this file the same session.
