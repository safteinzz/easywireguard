# The list-shaped README screenshots, rendered by VHS instead of taken by hand.
# No terminal to size, no cropping: the frame below is the frame of every shot,
# and a rerun after a UI change replaces them all identically.
#
#   cd demo && vhs shots.tape
#
# 125 columns by 15 rows: wide enough that the Mesh tab's hint line does not
# wrap (it is the longest thing the toolbox draws), short enough that the list
# is not swimming in an empty pane. The overlays that need a taller terminal -
# the QR, the inspector - are in overlays.tape, at the size they actually need.
#
# Files land straight in readme-assets/. Run one tape at a time: two of them
# share demo/home and would tear each other's fixtures down mid-take.

# VHS always records a video; with every frame hidden there would be nothing to
# encode and it stops with "no frames". This one is a throwaway.
Output "/tmp/vhs-ewg-shots-throwaway.gif"

Set Shell bash
# A nerd font, or the prompt's glyphs draw as boxes.
Set FontFamily "JetBrainsMono NF"
Set Theme "Catppuccin Mocha"
Set FontSize 15
Set Width 1320
Set Height 360
Set Padding 20

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
Type "ewg"
Enter
Sleep 2s
# Land on `mesh`, the interface that is both up and enabled at boot - the row
# the legend in the README is about.
Down
Sleep 500ms
Show
Sleep 500ms

# ---- Interfaces ----------------------------------------------------------
Screenshot ../readme-assets/interfaces.png
Sleep 700ms

# ---- Mesh ----------------------------------------------------------------
Hide
Type "l"
Sleep 500ms
# vps, its two spokes, hq, its one: land on `phone`, the node the QR shot uses.
Down
Down
Sleep 800ms
Show
Sleep 500ms
Screenshot ../readme-assets/mesh.png
Sleep 700ms

# ---- the node wizard -----------------------------------------------------
# Walked to the last field, so the frame shows the whole form filled in: type,
# name, the address it suggested, the hub picker, and where the key goes.
Hide
Type "c"
Sleep 500ms
Enter
Type "guest-phone"
Enter
Enter
Type "10.10.1.1"
Enter
Enter
Enter
Sleep 800ms
Show
Sleep 500ms
Screenshot ../readme-assets/wizard.png
Sleep 700ms

# Leave the stage as it was found: the wizard never submitted, so nothing was
# written, but the tape still closes the toolbox rather than killing it.
Hide
Escape
Sleep 300ms
Type "q"
Sleep 500ms
Type "exit"
Enter
Sleep 1s
