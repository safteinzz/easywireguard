# The two screenshots that need a taller terminal than the list shots: the QR
# and the inspector. Same rig, same fixtures, a frame sized for what is being
# drawn rather than for the list behind it.
#
#   cd demo && vhs overlays.tape
#
# A QR of a real config is 69 modules across, so even at two modules per row it
# needs about 71x37 - below that the toolbox refuses to draw a clipped, un-
# scannable code and says so instead. 125x39 clears it and leaves the list
# visible around the overlay, which is what makes it read as "in the app".
#
# Run one tape at a time: the tapes share demo/home.

Output "/tmp/vhs-ewg-overlays-throwaway.gif"

Set Shell bash
Set FontFamily "JetBrainsMono NF"
Set Theme "Catppuccin Mocha"
Set FontSize 15
Set Width 1320
Set Height 860
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

# ---- inspect -------------------------------------------------------------
# `mesh` is the config generated for the `laptop` node, so its peers are the
# nodes the Mesh tab lists, and it is up - which is what adds the live readout.
Down
Sleep 300ms
Type "i"
Sleep 1s
Show
Sleep 500ms
Screenshot ../readme-assets/inspect.png
Sleep 700ms

# ---- QR ------------------------------------------------------------------
Hide
Escape
Sleep 300ms
Type "l"
Sleep 300ms
Down
Down
Sleep 300ms
Enter
Sleep 1.5s
Show
Sleep 500ms
Screenshot ../readme-assets/qr.png
Sleep 700ms

Hide
Escape
Sleep 300ms
Type "q"
Sleep 500ms
Type "exit"
Enter
Sleep 1s
