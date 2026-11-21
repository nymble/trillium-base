#!/bin/sh
# Open the Trillium Base (pilab) desktop: SSH tunnel to the Pi's local-only VNC, then TigerVNC viewer
lsof -nP -iTCP:5901 -sTCP:LISTEN >/dev/null 2>&1 || ssh -f -o ExitOnForwardFailure=yes -L 5901:127.0.0.1:5900 lab@pilab.local sleep 30
nohup /Applications/TigerVNC.app/Contents/MacOS/vncviewer localhost::5901 >/tmp/pilab-vnc.log 2>&1 &
sleep 2
osascript -e 'tell application "Terminal" to close (every window whose name contains "pilab-desktop")' >/dev/null 2>&1 &
