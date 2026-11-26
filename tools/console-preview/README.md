<!-- Copyright (C) 2026 Paul Lambert. SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial -->
# Console preview

Look at the Trillium Console, and check it for accessibility, without a device attached.

1. On the device: `sh snapshot-api.sh ./api-snap`, then copy `api-snap/` to your computer.
   The snapshot holds that device's addresses and names. Keep it out of git.
2. `node mock-server.js ../../rust/crates/pilabd/web ./api-snap 8411` serves the web files with the
   snapshot as the API (writes are accepted and ignored).
3. `npm i playwright-core axe-core`, then
   `CHROME=/path/to/chrome SNAP=./api-snap node shots.js http://127.0.0.1:8411 ./shots after`
   writes a PNG of each view at desktop, tall, iPad and phone sizes, and
   `node axe-check.js http://127.0.0.1:8411` lists WCAG 2 A/AA findings per view and width.
