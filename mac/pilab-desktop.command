#!/bin/sh
# Copyright (C) 2026 Paul Lambert
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
# Open the Trillium Base desktop from a Mac: SSH tunnel to the device's local-only VNC
# (127.0.0.1:5900), then the TigerVNC viewer. Set TRILLIUM_HOST to use another device.
HOST=${TRILLIUM_HOST:-lab@pilab.local}
lsof -nP -iTCP:5901 -sTCP:LISTEN >/dev/null 2>&1 || ssh -f -o ExitOnForwardFailure=yes -L 5901:127.0.0.1:5900 "$HOST" sleep 30
nohup /Applications/TigerVNC.app/Contents/MacOS/vncviewer localhost::5901 >/tmp/pilab-vnc.log 2>&1 &
sleep 2
osascript -e 'tell application "Terminal" to close (every window whose name contains "pilab-desktop")' >/dev/null 2>&1 &
