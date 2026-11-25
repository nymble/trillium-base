// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
const { chromium } = require("playwright-core");
const fs = require("fs"), path = require("path");
// node shots.js <base-url> <out-dir> <tag> [desktop tall ipad phone ipad-land]
// env: SNAP=<snapshot dir> VIEWS=a,b SCHEMES=dark,light FULL=1 PAIR=1 (pairing screen)
const [,, base, outDir, tag, ...rest] = process.argv;
const SNAP = process.env.SNAP || "./api-snap";
const sys = JSON.parse(fs.readFileSync(SNAP + "/system.json")); sys.t = "sys";
const status = JSON.parse(fs.readFileSync(SNAP + "/status.json"));
const views = process.env.VIEWS ? process.env.VIEWS.split(",") : ["overview", "sounds", "midimap", "scenes", "signals", "devices", "settings"];
const sizes = { desktop: [1440, 900], tall: [1440, 2200], ipad: [820, 1180], phone: [390, 844], "ipad-land": [1180, 820] };
const fake = `(() => {
  const status = ${JSON.stringify(status)}, sys = ${JSON.stringify(sys)};
  class FW { constructor() { this.readyState = 1; let n = 0; setTimeout(() => { this.onmessage && this.onmessage({ data: JSON.stringify(sys) }); }, 50);
    this.iv = setInterval(() => { n++; const s = { ...status, peak_db: [-22 + Math.sin(n / 5) * 6, -24 + Math.cos(n / 4) * 6], voices: 3, input: { ...status.input, peak_db: [-30, -34] } }; this.onmessage && this.onmessage({ data: JSON.stringify(s) }); }, 66);
    setInterval(() => this.onmessage && this.onmessage({ data: JSON.stringify(sys) }), 2000); }
    send() {} close() { clearInterval(this.iv); } }
  window.WebSocket = FW;
  localStorage.setItem("pilab.token", "mock");
})();`;
(async () => {
  fs.mkdirSync(outDir, { recursive: true });
  const b = await chromium.launch({ executablePath: process.env.CHROME || undefined, args: ["--no-sandbox"] });
  const only = rest.length ? rest : Object.keys(sizes);
  for (const sz of only) {
    const [w, h] = sizes[sz];
    for (const scheme of (process.env.SCHEMES || "dark").split(",")) {
      const ctx = await b.newContext({ viewport: { width: w, height: h }, colorScheme: scheme, deviceScaleFactor: 1 });
      await ctx.addInitScript(process.env.PAIR ? fake.replace('localStorage.setItem("pilab.token", "mock");', "") : fake);
      const pg = await ctx.newPage();
      pg.on("pageerror", (e) => console.log("PAGEERROR", sz, e.stack.split("\n").slice(0,3).join(" | ")));
      pg.on("console", (m) => { if (m.type() === "error") console.log("CONSOLE", sz, m.text()); });
      for (const v of views) {
        await pg.goto(base + "/#" + v); await pg.reload(); await pg.waitForTimeout(1200);
        await pg.screenshot({ path: path.join(outDir, `${tag}-${sz}${scheme === "dark" ? "" : "-" + scheme}-${v}.png`), fullPage: process.env.FULL === "1" });
      }
      await ctx.close();
    }
  }
  await b.close();
})();
