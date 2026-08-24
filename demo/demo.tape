# The tour at the top of the README, rendered by VHS. Nothing here is recorded
# off a screen: VHS replays these keystrokes in a headless terminal and draws
# the GIF itself, so it re-renders identically every release.
#
#   cd demo && vhs demo.tape
#
# One beat per thing the tool does, each finished before the next starts: look
# at an interface, toggle it, put a node in the mesh, hand it out, take it away
# again. The frame is 129x38 - the QR needs about 37 rows, and everything else
# is sized to that.
#
# Every interface here is a stand-in (see stage.sh): the toggles really do
# change what the next listing reports, without a kernel, a real device or root.
#
# Run one tape at a time: the tapes share demo/home.

Output "../readme-assets/demo.gif"

Set Shell bash
# A nerd font, or the prompt's glyphs draw as boxes.
Set FontFamily "JetBrainsMono NF"
Set Theme "Catppuccin Mocha"
Set FontSize 13
Set Width 1100
Set Height 700
Set Padding 20
Set Framerate 24
Set TypingSpeed 60ms

# ---- setup, off camera ---------------------------------------------------
Hide
Type "./stage.sh up >/dev/null 2>&1"
Enter
Sleep 3s
Type "./stage.sh shell"
Enter
Sleep 2s
Type "clear"
Enter
Sleep 500ms
Show

# ===========================================================================
# SCENE 1 - every config you have, and whether it is actually running
# ===========================================================================
Type "ewg"
Sleep 300ms
Enter
Sleep 1.5s

Down
Sleep 1.2s

# `i` is the "is this thing working" answer: the file, plus a live wg show with
# the handshake and the transfer when the interface is up.
Type "i"
Sleep 3.5s
Escape
Sleep 800ms

# ===========================================================================
# SCENE 2 - up, down, and start-on-boot
# ===========================================================================
Down
Sleep 800ms
Enter
Sleep 2s

# the boot flag is systemd's, and independent of whether it is up right now
Type "b"
Sleep 1.8s
Type "b"
Sleep 1.5s

Enter
Sleep 2s

# ===========================================================================
# SCENE 3 - the mesh behind it
# ===========================================================================
Type "l"
Sleep 1.5s
Down
Sleep 600ms
Down
Sleep 600ms
Down
Sleep 1.2s

# ===========================================================================
# SCENE 4 - a new node, keys and all, ending in something a phone can scan
# ===========================================================================
Type "c"
Sleep 1.2s
Enter
Sleep 300ms
Type "guest-phone"
Sleep 800ms
Enter
Sleep 600ms
Enter
Sleep 400ms
Type "10.10.1.1"
Sleep 800ms
Enter
Sleep 800ms
Enter
Sleep 600ms
Enter
Sleep 1.2s
Enter
Sleep 4s
Escape
Sleep 1.2s

# ===========================================================================
# SCENE 5 - or hand it to whatever manages the other end
# ===========================================================================
Type "E"
Sleep 1.5s
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 800ms
Enter
Sleep 2.5s
Escape
Sleep 800ms

# ===========================================================================
# SCENE 6 - and gone again
# ===========================================================================
Type "d"
Sleep 1.5s
Type "y"
Sleep 1.8s

# ===========================================================================
# SCENE 7 - every node's config, written out
# ===========================================================================
# The status line fades after a second and a half by design, so the beat that
# ends on it is the config itself: this is what the manifest turns into, one
# file per node, each peered with exactly what it should reach.
Type "g"
Sleep 2s
Up
Sleep 600ms
Type "i"
Sleep 4s
