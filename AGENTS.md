<!--
AI-ONLY DOCUMENT. This file exists to give an AI agent the COMPLETE operating picture for this repo. Optimize for completeness and precision for the agent, not for human readability. Humans read README.md instead. Do not remove detail to make this nicer, err toward more explicit, not less. FORMAT: machine-read, not a formatted human doc. Do NOT hard-wrap lines to a column width for readability; put each rule/point on ONE line, however long.
-->
# AGENTS.md

Working brief for an AI coding agent, not documentation for people (the README covers that): the rules, invariants and gotchas needed to change this project correctly without rediscovering them.

## Hard rules
- Commit, push, and publish only when the user says to ship; a mid-work commit is never the deliverable, because the user tests interactively first.
- Commit messages are short single-line conventional ones (`feat:`, `fix:`, `chore:`, ...), never with a `Co-Authored-By` trailer and never with a verbose body.
- Release flow, in this exact order: ask whether this shipment gets tests and write them only if the user says yes -> bump `version` in `Cargo.toml` -> `cargo fmt --check` clean, `cargo clippy-all` clean and `cargo test` green, which is also what refreshes `Cargo.lock` with the new version -> `cargo +1.88 msrv` clean, the only thing that proves the `rust-version` floor in `Cargo.toml` is real -> one commit -> `git push origin main` -> `cargo publish` (dry-run first, publishing is irreversible) -> tag only after publish succeeds with `git tag vX.Y.Z && git push origin --tags`; a tag must never point at a version that failed to publish, and the bump comes first because `cargo publish` fails on a `Cargo.lock` that still holds the old version.
- Tests are proposed at ship time and never before: the first step of the release flow is to ask the user, in plain words, whether this shipment gets tests, and they are written only on a yes, so the decision is always theirs but the question is never forgotten.
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
- `.nocommit/` (gitignored) holds reference material used only to inform work here - other projects, notes, drafts - and never ships; keep it out of anything user-facing (commit messages, code, comments, README, `--help`), since a reference to material nobody outside this machine can see means nothing to them and just clutters the record.
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
- `cargo fmt` formats the crate and `cargo fmt --check` fails when anything has drifted; the whole crate is rustfmt clean, so formatting is never a judgement call and never a review comment.
- `cargo +1.88 msrv` checks the crate against the `rust-version` floor it advertises (alias in `.cargo/config.toml`), and when the code starts needing a newer compiler both that floor and the toolchain in this line move together.
- Unit tests sit at the bottom of the source file they cover, end-to-end tests in `tests/cli.rs`; drive the built binary against temp dirs only, never real WireGuard interfaces, `/etc/wireguard`, or real keys.

## README assets
- Every screenshot and the demo GIF are rendered by [VHS](https://github.com/charmbracelet/vhs) from the committed rig in `demo/`, never captured by hand: `demo/stage.sh` builds the world, `shots.tape` renders the list-shaped stills, `overlays.tape` the two that need a taller terminal (the QR, the inspector), `demo.tape` the tour. Run one tape at a time (`cd demo && vhs shots.tape`) - they share the stage and would tear each other's fixtures down mid-take.
- A UI change means rerunning the tape whose shot it changed, not editing an image. Images land in `readme-assets/`, flat and lowercase, overwritten in place; the README links them by absolute raw URL because crates.io renders it on its own site, where a relative path resolves against the registry and breaks. Pushing a new image updates every rendered README with no release.
- `demo/stage.sh` stands `wg`, `wg-quick` and `systemctl` up as shims on the staged `PATH` so an interface can be toggled on camera. The tool is unmodified; only the world under it is invented, which is what keeps the rig inside the rule about never touching real interfaces. The stage lives under `$TMPDIR`, outside this working tree, so nothing in frame can pick up this repo's branch and dirty count.
- A leak here means a real config in a frame - an endpoint, the DNS behind a tunnel, a public IP, a key - and since crates.io and every mirror fetch README images live, one rendered is one published: the fix is yanking the release and rotating what was in the shot. The rig enforces against that rather than hoping: everything staged runs under `env -i` with the allowlist in `env_for_stage`, because an exported `EWG_DIR` beats the staged registry and would point every screenshot at the renderer's real `/etc/wireguard`. A username or a clock in frame is not that kind of leak; the demo shell replaces the real `~/.bashrc` for reproducibility, and here for a second reason too - this machine's rc runs `fastfetch`, which paints host, distro, kernel and uptime across the shot one missing `clear` from the GIF.
- The rig must not be able to damage the machine: it writes only inside the stage, runs nothing privileged, its `systemctl` stand-in shadows the real one on the staged `PATH` only, and `down` deletes solely a tree carrying the `.ewg-demo-stage` marker `up` wrote - so an `EWG_DEMO_HOME` pointed at a real directory, at `$HOME` or at a system dir gets a refusal, symlinks resolved first and `--one-file-system` behind that. Test the refusals after touching them, never just the happy path.
- Nothing in a frame is real: RFC 5737 / RFC 3849 addresses, `example.com` names, keys generated by the build under test and thrown away with the stage. Nothing about the rig belongs in the README - a reader installing from crates.io got a package with `demo/` excluded and could not run it anyway.
- The demo rig is standardised across every crate in this directory, and the three parts that make the frames match must stay identical: the staged shell wears the invented `user@host` prompt written by `write_demorc`, every tape sets `Set Theme "Catppuccin Mocha"` (VHS's own default is a near-black that is harsher to read than Catppuccin's `#1e1e2e`), and every tape sets `Set FontFamily "JetBrainsMono NF"`. It is not about hiding a username, which is no leak - the pictures are a build output, and one that comes out different on every machine that regenerates it is not reproducible.
- Everything staged runs under `env -i` with a complete allowlist, never the real environment plus overrides, because an exported variable nobody thought of is exactly how a real config dir, a real endpoint or a real key ends up in a frame. The rig also writes only inside the stage, so nothing of the renderer's is touched on disk.
- Teardown goes through `assert_safe_to_delete`, word-for-word the same function in every crate's rig: the stage path must be absolute, must not be a system directory or the renderer's home, must resolve through its symlinks to a tree carrying the marker file the build stamped, and must have nothing mounted under it, with `rm -rf --one-file-system` behind that. Test the refusals after touching them, never just the happy path - an sshfs mount inside a staged home, torn down with a plain `rm -rf`, has already deleted the dotfiles on the machine at the far end.

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
