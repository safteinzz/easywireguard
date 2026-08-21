<!--
AI-ONLY DOCUMENT. This file exists to give an AI agent the COMPLETE operating picture for this repo. Optimize for completeness and precision for the agent, not for human readability. Humans read README.md instead. Do not remove detail to make this nicer, err toward more explicit, not less. FORMAT: machine-read, not a formatted human doc. Do NOT hard-wrap lines to a column width for readability; put each rule/point on ONE line, however long.
-->
# AGENTS.md

Working brief for an AI coding agent, not documentation for people (the README covers that): the rules, invariants and gotchas needed to change this project correctly without rediscovering them.

## Hard rules
- Commit, push, and publish only when the user says to ship; a mid-work commit is never the deliverable, because the user tests interactively first.
- Commit messages are short single-line conventional ones (`feat:`, `fix:`, `chore:`, ...), never with a `Co-Authored-By` trailer and never with a verbose body.
- Release flow, in this exact order: write the regression tests for what is about to ship -> bump `version` in `Cargo.toml` -> `cargo clippy-all` clean and `cargo test` green, which is also what refreshes `Cargo.lock` with the new version -> one commit -> `git push origin main` -> `cargo publish` (dry-run first, publishing is irreversible) -> tag only after publish succeeds with `git tag vX.Y.Z && git push origin --tags`; a tag must never point at a version that failed to publish, and the bump comes first because `cargo publish` fails on a `Cargo.lock` that still holds the old version.
- Tests are written at ship time and only then: covering the behaviour that just settled is the first step of the release flow, so the suite grows once per release instead of once per commit.
- Never write a test for behaviour that has not shipped yet, because code that is not in the last release tag is still being designed, and a test pinning a shape that is about to change is how a suite starts lying.
- A test may only assert something the README or `--help` promises, or a pure-logic invariant (parsing, generation, path resolution, validation); never the shape of a private function and never the specific diff that was just made, since those rot on the next refactor and teach nothing about whether the program works.
- Removing a promise from the README removes its tests in the same commit.
- A test may only write inside a temp directory it deletes, never a real config, data, cache or content directory and never a fixed path, so a machine is left exactly as it was before the suite ran.
- Never drive the interface to test it: build it, say what changed and what to look at, and let the user run it, because they see the screen instantly while an agent driving a pty or a tmux pane is slow and wrong about what it looks like; logic that is not visual can still be checked directly from `tests/`.
- Never `cargo install` to test: run the release binary at `./target/release/ewg` directly, because installing replaces the binary on PATH with a work-in-progress build; install only when the user asks.
- `main` is protected: no force-push and no history rewrite, so a mistake is fixed with a forward commit.
- No em-dashes anywhere (code, comments, README, `--help`, crate description, commit messages, prose), because they read as AI-generated text; use `-` instead.
- Fix the root cause, and if a workaround must ship say the word "workaround" out loud so a silent patch never passes as a real fix; the same goes for lints, where an `#[allow]` is never the answer and the code it points at gets fixed or deleted.
- `TODO-LIST.md` (gitignored) holds one-line ideas, and the line is deleted when the idea ships.
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
- User-facing errors are lowercase, name things in backticks, and say what to do; never let a raw OS error reach the user.

## Build / lint / test
- `cargo build --release`, binary at `target/release/ewg`.
- `cargo clippy-all` is the lint pass, aliased in `.cargo/config.toml` to `clippy --release --all-targets -- -D warnings`; use it rather than a bare `cargo clippy`, which skips `tests/` and `examples/` and only warns where the release flow wants a failure.
- `cargo test`.
- Unit tests sit at the bottom of the source file they cover, end-to-end tests in `tests/cli.rs`; drive the built binary against temp dirs only, never real WireGuard interfaces, `/etc/wireguard`, or real keys.

## Overview
`ewg` (crate `easywireguard`) generates WireGuard key material and manages live interfaces, and from a single mesh manifest generates each node's `.conf` as "all peers minus itself". Built for full-mesh WireGuard (every node an equal peer, no central hub) plus day-to-day interface management, with no external `wg` binary needed for its core.
If this file contradicts the code, the code wins; fix this file the same session.

## Self-repair
If anything here contradicts the code, the code wins; fix AGENTS.md in the same session you notice the drift.
