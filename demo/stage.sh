#!/usr/bin/env bash
# A staged machine for the README screenshots and the demo GIF: invented configs,
# an invented mesh, throwaway keys, and stand-in `wg` / `wg-quick` / `systemctl`
# so interfaces can be toggled on camera. Nothing here touches your real
# /etc/wireguard, your real interfaces, your real keys or your real systemd -
# every path is redirected into a staged home under $TMPDIR, and the three shims
# shadow the real binaries only inside the staged shell's PATH.
#
#   ./stage.sh up     build the fixtures
#   ./stage.sh run    launch the toolbox against them (this is what you shoot)
#   ./stage.sh shell  a shell where `ewg` is this build, for a CLI shot
#   ./stage.sh down   delete the stage
#
# Every address is from a range reserved for documentation (RFC 5737, RFC 3849),
# every hostname is example.com (RFC 2606), and every key is generated here by
# this build and thrown away with the stage. There is nothing real to leak, and
# nothing to configure: the tool's core is pure Rust, so unlike a rig for an ssh
# tool there is no machine to point it at and no demo/.env to fill in.
#
# Two things this rig must never do, both enforced below rather than hoped for:
# put anything of yours in a frame that would cost you to publish, and touch
# anything of yours on disk. The second one first: it writes only inside the stage, it never runs a privileged
# command, its `systemctl` stand-in cannot enable a real unit because it shadows
# the real one only on the staged PATH, and `down` refuses to delete any tree
# that is not carrying the marker file this script writes - so pointing
# $EWG_DEMO_HOME at a directory of yours gets a refusal, not a deletion.
#
# The leak to actually worry about is a real config reaching a frame: an endpoint
# peers dial, the DNS behind a tunnel, a public IP, a key. One rendered is one
# published - README images are fetched live by crates.io and every mirror - and
# the fix is yanking the release and rotating what was in the shot. So every
# command runs under `env -i` with the allowlist in env_for_stage below: an
# $EWG_DIR exported in your shell would otherwise beat the staged registry and
# point every screenshot at your real /etc/wireguard, and no override list can
# be trusted to have thought of everything. A username or a clock in the frame
# is not that, and is not worth a line of code.
#
# Why shims and not a real interface: bringing one up needs root and a kernel
# module, would show a real device in the frame, and `wg show` on a tunnel with
# no peers is an empty box. The tool is unmodified - it shells out to `wg`,
# `wg-quick` and `systemctl` exactly as it always does; only the world under it
# is invented, which is the same trick as a socket that listens and never
# accepts.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EWG="$HERE/../target/release/ewg"

# The stage lives outside the repo on purpose. A staged home inside demo/ sits in
# this git working tree, and a prompt with a git segment then reports THIS repo's
# branch and dirty count in every frame of the GIF - dev-machine noise the reader
# cannot make sense of. From here the prompt's `~` is the fixture.
STAGE="${EWG_DEMO_HOME:-${TMPDIR:-/tmp}/ewg-demo-home}"
BIN="$STAGE/.bin"

# Written by `up`, required by `down`. This script deletes a directory tree
# recursively, and the path is overridable, which is exactly the shape of thing
# that eats a home directory - so it refuses to delete anything that is not
# carrying this file, i.e. anything it did not build itself.
MARKER=".ewg-demo-stage"

# Where the shims keep the machine's invented state: which interfaces are up,
# which start at boot, and what `wg show <name>` prints for each.
STATE="$STAGE/.state"

# The config dir the registry points at. `~/wg`, not /etc/wireguard, so the
# frame never shows a path that needs root - and so the screenshots prove the
# registry works, which is the whole point of `ewg dir`.
CONFDIR="$STAGE/wg"

# The complete environment anything staged runs in. Used with `env -i`, so this
# list is not "the real environment plus overrides" - it is everything there is.
# HOME alone would not be enough (a surviving XDG_CONFIG_HOME writes the registry
# back into the real one), and neither would overriding: an exported EWG_DIR
# would still win and point the tool at your actual configs.
env_for_stage() {
  echo "HOME=$STAGE" \
       "XDG_CONFIG_HOME=$STAGE/.config" \
       "XDG_DATA_HOME=$STAGE/.local/share" \
       "XDG_STATE_HOME=$STAGE/.local/state" \
       "XDG_CACHE_HOME=$STAGE/.cache" \
       "EWG_REGISTRY=$STAGE/.config/ewg/dirs.toml" \
       "EWG_DEMO_STATE=$STATE" \
       "EWG_NO_SUDO=1" \
       "PATH=$BIN:/usr/local/bin:/usr/bin:/bin" \
       "TERM=${TERM:-xterm-256color}" \
       "COLORTERM=truecolor" \
       "LANG=C.UTF-8"
}

# --- the stand-ins --------------------------------------------------------
# Three tiny scripts, each answering only the calls ewg actually makes. They
# read and write $EWG_DEMO_STATE, so a toggle in the TUI really does change what
# the next listing reports - the tape presses ↵ and the row goes green, with no
# kernel involved.
write_shims() {
  mkdir -p "$BIN"

  cat > "$BIN/wg" <<'EOF'
#!/usr/bin/env bash
# Stand-in for `wg`: only `show interfaces` and `show <name>`, which is all ewg asks.
set -euo pipefail
state="${EWG_DEMO_STATE:?not staged}"
[ "${1:-}" = show ] || exit 1
case "${2:-interfaces}" in
  interfaces) tr '\n' ' ' < "$state/up"; echo ;;
  *) cat "$state/show/$2" 2>/dev/null || exit 1 ;;
esac
EOF

  cat > "$BIN/wg-quick" <<'EOF'
#!/usr/bin/env bash
# Stand-in for `wg-quick up|down <path>`: moves a name in and out of the up list.
set -euo pipefail
state="${EWG_DEMO_STATE:?not staged}"
name="$(basename "${2:?}" .conf)"
case "${1:?}" in
  up)   grep -qxF "$name" "$state/up" || echo "$name" >> "$state/up" ;;
  down) grep -vxF "$name" "$state/up" > "$state/up.tmp" || true
        mv "$state/up.tmp" "$state/up" ;;
  *)    exit 1 ;;
esac
EOF

  cat > "$BIN/systemctl" <<'EOF'
#!/usr/bin/env bash
# Stand-in for systemd: is-enabled / enable / disable of wg-quick@<name>.
set -euo pipefail
state="${EWG_DEMO_STATE:?not staged}"
unit="${2:?}"; name="${unit#wg-quick@}"; name="${name%.service}"
case "${1:?}" in
  is-enabled) if grep -qxF "$name" "$state/boot"; then echo enabled; else echo disabled; fi ;;
  enable)     grep -qxF "$name" "$state/boot" || echo "$name" >> "$state/boot" ;;
  disable)    grep -vxF "$name" "$state/boot" > "$state/boot.tmp" || true
              mv "$state/boot.tmp" "$state/boot" ;;
  *)          exit 1 ;;
esac
EOF

  # The build under test, reachable as the command a reader would type.
  ln -sf "$(cd "$(dirname "$EWG")" && pwd)/ewg" "$BIN/ewg"
  chmod +x "$BIN/wg" "$BIN/wg-quick" "$BIN/systemctl"
}

# --- the fixtures ---------------------------------------------------------

# A throwaway keypair from this build. Sets $PRIV and $PUB.
keypair() {
  local out; out="$("$EWG" key)"
  PRIV="${out%$'\n'*}"; PRIV="${PRIV#PrivateKey = }"
  PUB="${out#*$'\n'}";  PUB="${PUB#PublicKey  = }"
}

# The mesh the Mesh tab shows: two hubs, three spokes nested under them. `vps` is
# a full-tunnel exit, `hq` carries a LAN so its spoke reaches the office subnet.
write_manifest() {
  local m="$STAGE/mesh.toml"
  keypair; VPS_PUB=$PUB
  "$EWG" mesh -m "$m" add vps --address 10.10.1.1/24 --pubkey "$VPS_PUB" \
    --endpoint vpn.example.com:51820 --allowed-ips 0.0.0.0/0 --keepalive 25 > /dev/null
  keypair; HQ_PUB=$PUB
  "$EWG" mesh -m "$m" add hq --address 10.10.1.2/24 --pubkey "$HQ_PUB" \
    --endpoint hq.example.com:51820 --allowed-ips "10.10.1.2/32,192.0.2.0/24" > /dev/null
  keypair; LAPTOP_PUB=$PUB; LAPTOP_PRIV=$PRIV
  "$EWG" mesh -m "$m" add laptop --address 10.10.1.3/24 --pubkey "$LAPTOP_PUB" \
    --private "$LAPTOP_PRIV" --hub vps > /dev/null
  keypair; PHONE_PUB=$PUB
  "$EWG" mesh -m "$m" add phone --address 10.10.1.4/24 --pubkey "$PHONE_PUB" \
    --private "$PRIV" --hub vps --dns 10.10.1.1 > /dev/null
  keypair; TABLET_PUB=$PUB
  "$EWG" mesh -m "$m" add tablet --address 10.10.1.5/24 --pubkey "$TABLET_PUB" \
    --private "$PRIV" --hub hq > /dev/null
}

# The `.conf` files the Interfaces tab lists. `mesh.conf` is the real generated
# output for the `laptop` node, so inspecting it shows the peers the Mesh tab
# just showed - one machine, two views.
write_confs() {
  mkdir -p "$CONFDIR"
  local gen="$STAGE/.gen"
  "$EWG" mesh -m "$STAGE/mesh.toml" gen -o "$gen" 2> /dev/null
  cp "$gen/laptop.conf" "$CONFDIR/mesh.conf"
  rm -rf "$gen"

  keypair
  cat > "$CONFDIR/work.conf" <<EOF
[Interface]
PrivateKey = $PRIV
Address    = 198.51.100.24/32
DNS        = 198.51.100.1

[Peer]
PublicKey  = $VPS_PUB
Endpoint   = gateway.example.com:51820
AllowedIPs = 198.51.100.0/24
PersistentKeepalive = 25
EOF

  keypair
  cat > "$CONFDIR/travel.conf" <<EOF
# Pasted straight out of a provider's dashboard, validated on the way in.
[Interface]
PrivateKey = $PRIV
Address    = 203.0.113.7/32
DNS        = 203.0.113.1

[Peer]
PublicKey  = $HQ_PUB
Endpoint   = exit-01.example.com:51820
AllowedIPs = 0.0.0.0/0, ::/0
EOF

  keypair; NAS_PUB=$PUB
  cat > "$CONFDIR/nas.conf" <<EOF
[Interface]
PrivateKey = $PRIV
Address    = 198.51.100.9/32

[Peer]
PublicKey  = $NAS_PUB
Endpoint   = nas.example.com:51820
AllowedIPs = 198.51.100.0/24
PersistentKeepalive = 25
EOF

  keypair; LAB_PUB=$PUB
  cat > "$CONFDIR/lab.conf" <<EOF
[Interface]
PrivateKey = $PRIV
Address    = 192.0.2.10/24
ListenPort = 51821

[Peer]
PublicKey  = $LAB_PUB
AllowedIPs = 192.0.2.0/24
EOF
}

# Which interfaces are up, which start at boot, and what a live `wg show` says
# about them. `mesh` is up and starts at boot, `lab` is up only, `nas` is enabled
# but currently down: the two markers are independent, and a listing that is all
# one state teaches nothing about either.
seed_state() {
  mkdir -p "$STATE/show"
  printf 'lab\nmesh\n' > "$STATE/up"
  printf 'mesh\nnas\n' > "$STATE/boot"

  # Handshake ages are what `wg show` renders as "1 minute, 12 seconds ago", so
  # they are what makes the readout look like a tunnel that is actually carrying
  # traffic rather than a fresh, empty one.
  cat > "$STATE/show/mesh" <<EOF
interface: mesh
  public key: $LAPTOP_PUB
  private key: (hidden)
  listening port: 51820

peer: $VPS_PUB
  endpoint: 192.0.2.1:51820
  allowed ips: 0.0.0.0/0
  latest handshake: 1 minute, 12 seconds ago
  transfer: 4.21 MiB received, 812.44 KiB sent
  persistent keepalive: every 25 seconds
EOF

  cat > "$STATE/show/lab" <<EOF
interface: lab
  public key: $LAB_PUB
  private key: (hidden)
  listening port: 51821

peer: $LAB_PUB
  endpoint: 192.0.2.20:51821
  allowed ips: 192.0.2.0/24
  latest handshake: 27 seconds ago
  transfer: 1.03 GiB received, 96.18 MiB sent
EOF
}

up() {
  down_quiet
  mkdir -p "$STAGE" "$STAGE/.config"
  # Stamp it before anything else, so a later `down` can prove this tree is ours.
  : > "$STAGE/$MARKER"
  write_shims
  write_manifest
  write_confs
  seed_state
  # Register the config dir, so the tool finds it the way a user's would.
  env -i $(env_for_stage) "$EWG" dir add "$CONFDIR" > /dev/null
  # An empty stand-in for anything a shell rc sources out of the real home.
  mkdir -p "$STAGE/.cargo" && : > "$STAGE/.cargo/env"
  echo "staged in $STAGE"
  echo
  echo "  ./stage.sh run    open the toolbox against it"
  echo "  ./stage.sh shell  a shell where \`ewg\` is this build"
  echo "  ./stage.sh down   tear it all down"
}

# ---------------------------------------------------------------------------
# the teardown guard - identical in every crate's rig
# ---------------------------------------------------------------------------
# A rig is a convenience script with a recursive delete in it, run half
# attentively while thinking about something else, against a path some scenario
# may have mounted a remote filesystem onto. Both halves of that have already
# happened in this workflow: a stage path that pointed somewhere real and was
# deleted because the script trusted its own variable, and an sshfs mount inside
# a staged home torn down with `rm -rf`, which walked through the mountpoint and
# deleted the dotfiles on the machine at the far end. So the delete is proved
# rather than trusted.
refuse() { echo "REFUSING to delete $STAGE: $1" >&2; exit 1; }

assert_safe_to_delete() {
  case "$STAGE" in
    /*) ;;
    *) refuse "the stage path must be absolute" ;;
  esac
  # Resolve symlinks first: a link pointing the stage at something real must not
  # let a delete through on the strength of a harmless-looking path.
  local real
  real="$(cd "$STAGE" && pwd -P)" || refuse "cannot resolve the path"
  case "$real" in
    / | /home | /root | /usr | /etc | /var | /opt | /srv | /boot | /tmp)
      refuse "that is a system directory" ;;
  esac
  [ "$real" = "$HOME" ] && refuse "that is your home directory"
  case "$HOME/" in
    "$real"/*) refuse "your home directory is inside it" ;;
  esac
  # The real gate: only ever delete a tree this script built and stamped.
  [ -f "$real/$MARKER" ] || refuse "no \`$MARKER\` in it, so this script did not build it"
  # Unmount anything under it, longest path first, then check again: a recursive
  # delete walks straight through a mountpoint and removes the far side.
  local mp
  while read -r mp; do
    [ -n "$mp" ] || continue
    echo "unmounting $mp"
    fusermount -u "$mp" 2> /dev/null || umount "$mp" 2> /dev/null || true
  done < <(awk -v s="$real/" '$2 ~ "^"s {print length($2), $2}' /proc/mounts |
             sort -rn | cut -d' ' -f2-)
  if awk -v s="$real/" '$2 ~ "^"s {found=1} END {exit !found}' /proc/mounts; then
    refuse "something is still mounted under it; unmount it by hand and rerun"
  fi
}

down_quiet() {
  [ -d "$STAGE" ] || return 0
  assert_safe_to_delete
  # --one-file-system as a second net, in case the mount check was wrong.
  rm -rf --one-file-system "$STAGE"
}

# ---------------------------------------------------------------------------
# the shell in frame - identical in every crate's rig
# ---------------------------------------------------------------------------
# The prompt is invented, and deliberately not the renderer's own. Sourcing a
# real ~/.bashrc paints a different picture on every machine that regenerates
# the assets, which defeats the point of keeping the rig in the repo: these
# images are a build output, and a build output that depends on whose machine
# ran it is not reproducible. A username is not a leak, but `user@host` is the
# same for everyone, and it is the same string in all six rigs so the frames
# match. No tape sets a theme either, so every frame is VHS's default black.
write_demorc() {
  cat > "$STAGE/.demorc" <<'EOF'
PS1='\[\e[38;5;114m\]user@host\[\e[0m\]:\[\e[38;5;110m\]\w\[\e[0m\]\$ '
unset PROMPT_COMMAND
HISTFILE=
clear
EOF
}

# A shell that finds this build as `ewg` and the shims as `wg`/`wg-quick`/
# `systemctl`. It starts in the staged home, so `~` on screen is the fixture and
# `mesh.toml` resolves beside it. Sourcing the real ~/.bashrc is out for the
# reason above and for one of its own: this machine's runs `fastfetch`, which
# paints host, distro, kernel and uptime across the shot one missing `clear`
# from the GIF, and under `env -i` it would need its own variables threaded back
# in as well.
open_shell() {
  write_demorc
  (cd "$STAGE" && env -i $(env_for_stage) \
    bash --noprofile --rcfile "$STAGE/.demorc" -i)
}

case "${1:-up}" in
  up)    up ;;
  # From inside the staged home: the TUI reads ./mesh.toml and `g` writes ./out,
  # both relative to the working directory.
  run)   (cd "$STAGE" && env -i $(env_for_stage) "$EWG") ;;
  shell) open_shell ;;
  down)  down_quiet; echo "torn down" ;;
  *)     echo "usage: $0 [up|run|shell|down]" >&2; exit 2 ;;
esac
