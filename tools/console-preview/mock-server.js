// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
// Mock of the pilabd console server for visual review: the web directory plus replayed API
// snapshots (see snapshot-api.sh). Writes are accepted and ignored.
//   node mock-server.js <web-dir> <snapshot-dir> [port]
const http = require("http"), fs = require("fs"), path = require("path");
const WEB = process.argv[2], SNAP = process.argv[3], PORT = +process.argv[4] || 8411;
const info = JSON.parse(fs.readFileSync(path.join(SNAP, "info.json")));
const mime = { ".html": "text/html", ".css": "text/css", ".js": "text/javascript", ".svg": "image/svg+xml", ".woff2": "font/woff2", ".json": "application/json", ".png": "image/png", ".ico": "image/x-icon", ".webmanifest": "application/manifest+json" };
const snap = (n) => { try { return fs.readFileSync(path.join(SNAP, n + ".json")); } catch { return null; } };
function tpl(s) { return s.replace(/{{PRODUCT_NAME}}/g, info.product).replace(/{{DEVICE_NAME}}/g, info.device).replace(/{{TAGLINE}}/g, info.tagline).replace(/{{VERSION}}/g, info.version).replace(/{{COMMIT}}/g, info.commit).replace(/{{BUILD}}/g, info.build); }
http.createServer((req, res) => {
  const u = new URL(req.url, "http://x"); let p = u.pathname;
  if (p.startsWith("/api/v1/")) {
    const e = p.slice(8);
    let body = null;
    if (req.method === "GET") {
      body = snap(e.replace(/\//g, "_")) || (e.startsWith("scene/") || e.startsWith("library/") || e.startsWith("devices/") ? null : snap(e.split("/")[0]));
      if (e === "scenes") body = snap("scenes");
      const sw = (k) => Array.from({ length: 450 }, (_, i) => Math.sin(i / 450 * 6.283 * k) * 0.6 + Math.sin(i / 450 * 6.283 * k * 3) * 0.2);
      if (/slots\/\d+\/wave/.test(e)) body = Buffer.from(JSON.stringify({ points: sw(1) }));
      else if (e.startsWith("library/preview")) body = Buffer.from(JSON.stringify({ points: sw(1).slice(0, 64) }));
      else if (/library\/akwf\/[^/]+\/frames/.test(e)) body = Buffer.from(JSON.stringify({ frames: Array.from({ length: 12 }, (_, k) => sw(k + 1).slice(0, 64)) }));
      else if (/^library\/akwf\/[^/]+$/.test(e)) body = Buffer.from(JSON.stringify({ files: Array.from({ length: 12 }, (_, k) => ({ file: "AKWF_" + String(k + 1).padStart(4, "0") + ".wav", path: "x/AKWF_" + k + ".wav" })) }));
      else if (/^devices\/[^/]+\/telemetry/.test(e)) body = Buffer.from(JSON.stringify({ frames: [] }));
      if (!body) body = Buffer.from("{}");
    } else { body = Buffer.from('{"ok":true}'); }
    res.writeHead(200, { "content-type": "application/json" }); return res.end(body);
  }
  if (p === "/" ) p = "/index.html";
  const f = path.join(WEB, path.normalize(p));
  if (!f.startsWith(WEB) || !fs.existsSync(f) || fs.statSync(f).isDirectory()) { res.writeHead(404); return res.end("nf"); }
  const ext = path.extname(f); let data = fs.readFileSync(f);
  if (ext === ".html" || ext === ".js" || ext === ".css") data = Buffer.from(tpl(data.toString()));
  res.writeHead(200, { "content-type": mime[ext] || "application/octet-stream" }); res.end(data);
}).listen(PORT, "127.0.0.1", () => console.log("mock on", PORT));
