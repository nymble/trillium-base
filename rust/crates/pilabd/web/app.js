// Copyright (C) 2026 Nymble. SPDX-License-Identifier: GPL-3.0-or-later
// pilab console web UI. Vanilla JS, no build step; served from the pilabd binary.
"use strict";

const API = "/api/v1";
const PRODUCT = document.body.dataset.product || "pilab console";
const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => Array.from(el.querySelectorAll(s));
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const NOTE = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const noteName = (n) => NOTE[n % 12] + (Math.floor(n / 12) - 1);
const fmtDb = (d) => (d <= -119 ? "−∞" : (d >= 0 ? "+" : "") + d.toFixed(1)) + " dB";
const WAVES = ["sine", "saw", "square", "pulse", "triangle", "table", "sampler"];
const WAVE_LABEL = { table: "wavetable" };

const S = {
  token: localStorage.getItem("pilab.token") || "",
  view: (location.hash || "#overview").slice(1),
  info: null, sys: null, status: null, scene: null, dirty: false, inputs: [],
  scenes: [], settings: null, graph: null, akwf: null, akwfGroup: null, akwfFiles: [], akwfFrames: null, cat: null, wtQuery: "", wtTag: "",
  fav: JSON.parse(localStorage.getItem("pilab.fav") || "[]"),
  selSlot: 1, midiLog: [], samples: null, presets: null, browserTab: localStorage.getItem("pilab.tab") || "samples",
  out: localStorage.getItem("pilab.out") || "dac", prevSel: localStorage.getItem("pilab.previewOnSelect") === "1", activity: {}, kbdOct: 4, kbdTarget: "route", ws: null,
};

// ------------------------------------------------------------------ API
async function api(method, path, body) {
  const r = await fetch(API + path, {
    method,
    headers: { "Authorization": "Bearer " + S.token, ...(body !== undefined ? { "Content-Type": "application/json" } : {}) },
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  if (r.status === 401) { showPair("The pairing token was not accepted."); throw new Error("unauthorized"); }
  const ct = r.headers.get("content-type") || "";
  const data = ct.includes("json") ? await r.json() : await r.text();
  if (!r.ok) throw new Error((data && data.error) || r.statusText);
  return data;
}
const get = (p) => api("GET", p);
const post = (p, b = {}) => api("POST", p, b);
const put = (p, b) => api("PUT", p, b);

let toastTimer;
function toast(msg, err = false) {
  const t = $("#toast");
  t.textContent = msg; t.className = "toast" + (err ? " err" : "");
  clearTimeout(toastTimer); toastTimer = setTimeout(() => t.classList.add("hidden"), err ? 6000 : 3200);
}
const fail = (e) => toast(e.message || String(e), true);

// ------------------------------------------------------------------ audition
// Plays a ~1.5 s phrase: on the DAC (Pi headphone/line out) at the current master, or
// rendered by pilabd to a small WAV and played here with Web Audio.
let audCtx, audSrc;
async function audition(src) {
  const q = Object.entries(src).map(([k, v]) => `${k}=${encodeURIComponent(v)}`).join("&");
  try {
    if (S.out === "browser") {
      toast("Rendering…");
      const r = await fetch(`${API}/audition/render?${q}`, { headers: { "Authorization": "Bearer " + S.token } });
      if (!r.ok) throw new Error((await r.json().catch(() => ({}))).error || r.statusText);
      audCtx = audCtx || new AudioContext();
      const buf = await audCtx.decodeAudioData(await r.arrayBuffer());
      if (audSrc) try { audSrc.stop(); } catch (e) {}
      audSrc = audCtx.createBufferSource(); audSrc.buffer = buf; audSrc.connect(audCtx.destination); audSrc.start();
      toast(`Playing in this browser (rendered at the Pi's master level)`);
    } else {
      const r = await post("/audition/play", src);
      toast(`Playing on ${r.label} at ${fmtDb(r.master_db)}: ${r.playing} · ${r.phrase}`);
    }
  } catch (e) { fail(e); }
}
const playBtn = (attrs, title = "Play") => `<button class="btn ghost small play" ${attrs} title="${esc(title)}" aria-label="${esc(title)}">▶</button>`;
function bindPlay(root = document) {
  $$("[data-play]", root).forEach((b) => b.addEventListener("click", (e) => {
    e.stopPropagation();
    const [k, ...v] = b.dataset.play.split(":");
    audition({ [k]: k === "slot" ? +v.join(":") : v.join(":") });
  }));
}
function outputSeg() {
  return `<div class="row tight"><span class="muted small">Play on</span><div class="seg" id="out-seg">
    <button data-o="dac" class="${S.out === "dac" ? "on" : ""}" title="Pi DAC: headphone / line out, at the current master">DAC</button>
    <button data-o="browser" class="${S.out === "browser" ? "on" : ""}" title="Render on the Pi, play in this browser">This browser</button></div>
    <label class="row tight muted small" title="Play a short phrase whenever you select a sound or table"><span>Preview on select</span><label class="switch"><input type="checkbox" id="prev-sel" ${S.prevSel ? "checked" : ""}><span></span></label></label></div>`;
}
function bindOutput(rerender) {
  $$("#out-seg button").forEach((b) => b.addEventListener("click", () => { S.out = b.dataset.o; localStorage.setItem("pilab.out", S.out); rerender(); }));
  const ps = $("#prev-sel"); if (ps) ps.addEventListener("change", (e) => { S.prevSel = e.target.checked; localStorage.setItem("pilab.previewOnSelect", S.prevSel ? "1" : "0"); });
}

// ------------------------------------------------------------------ pairing
function showPair(msg) {
  $("#app").classList.add("hidden");
  $("#pair").classList.remove("hidden");
  $("#pair-msg").textContent = msg || "";
  if (S.ws) { S.ws.onclose = null; S.ws.close(); S.ws = null; }
}
$("#pair-form").addEventListener("submit", async (ev) => {
  ev.preventDefault();
  S.token = $("#pair-token").value.trim();
  try {
    await post("/pair/check");
    localStorage.setItem("pilab.token", S.token);
    $("#pair").classList.add("hidden");
    start();
  } catch (e) { $("#pair-msg").textContent = "Not accepted. Check the token and try again."; }
});

// ------------------------------------------------------------------ new version check
// The page knows the build it was served with; if the server reports a different build
// (redeployed while this tab was open), offer a reload.
const PAGE_BUILD = (document.querySelector('meta[name="pilab-build"]') || {}).content || "";
function checkBuild(info) {
  if (!info || !info.build || !PAGE_BUILD || PAGE_BUILD.includes("{{")) return;
  if (info.build === PAGE_BUILD || S.newverDismissed === info.build) return;
  $("#newver-detail").textContent = `${PAGE_BUILD.split("-")[0]} → ${(info.commit || info.build).slice(0, 7)}`;
  $("#newver").classList.remove("hidden");
  S.newverBuild = info.build;
}
async function pollBuild() {
  try { checkBuild(await (await fetch(API + "/info", { cache: "no-store" })).json()); } catch (e) { /* offline */ }
}
$("#newver-reload").addEventListener("click", () => location.reload());
$("#newver-close").addEventListener("click", () => { S.newverDismissed = S.newverBuild; $("#newver").classList.add("hidden"); });
setInterval(pollBuild, 60000);
document.addEventListener("visibilitychange", () => { if (!document.hidden) pollBuild(); });

// ------------------------------------------------------------------ boot
async function boot() {
  try { S.info = await (await fetch(API + "/info", { cache: "no-store" })).json(); } catch (e) { S.info = null; }
  checkBuild(S.info);
  if (S.info) {
    $("#pair-device").textContent = `${S.info.hostname} · device ${S.info.device_id} · v${S.info.version}`;
    $("#foot-ver").textContent = `${S.info.product} v${S.info.version} · ${S.info.device_id}`;
  }
  if (!S.token) return showPair();
  try { await post("/pair/check"); start(); } catch (e) { if (e.message !== "unauthorized") showPair("Cannot reach the device: " + e.message); }
}

async function inlineIcons() {
  for (const img of $$(".side a img")) {
    try {
      const svg = await (await fetch(img.src)).text();
      const span = document.createElement("span");
      span.innerHTML = svg;
      img.replaceWith(span.firstElementChild);
    } catch (e) { /* keep the img */ }
  }
}

async function start() {
  $("#app").classList.remove("hidden");
  $("#hostline").textContent = S.info ? `${S.info.hostname} · ${S.info.device_id}` : "";
  inlineIcons();
  buildKeyboard();
  connectWs();
  await refreshScene();
  try { S.sys = await get("/system"); } catch (e) {}
  render();
}

function connectWs() {
  const proto = location.protocol === "https:" ? "wss" : "ws";
  const ws = new WebSocket(`${proto}://${location.host}${API}/ws?token=${encodeURIComponent(S.token)}`);
  S.ws = ws;
  ws.onmessage = (m) => {
    const d = JSON.parse(m.data);
    if (d.t === "status") onStatus(d);
    else if (d.t === "sys") { S.sys = d; onSys(); }
  };
  ws.onclose = () => { S.ws = null; setTimeout(pollBuild, 3000); setTimeout(() => { if (!$("#app").classList.contains("hidden")) connectWs(); }, 1500); };
}

// ------------------------------------------------------------------ live header
function onStatus(d) {
  S.status = d;
  const pct = (db) => Math.max(0, Math.min(100, (db + 60) / 60 * 100));
  $("#m-l").style.width = pct(d.peak_db[0]) + "%";
  $("#m-r").style.width = pct(d.peak_db[1]) + "%";
  $("#st-voices").textContent = `${d.voices}/32`;
  $("#st-dsp").textContent = d.dsp_load ? d.dsp_load.toFixed(1) + "%" : "—";
  $("#st-xruns").textContent = d.xrun_rate > 0 ? `${d.xruns} (${d.xrun_rate}/s)` : d.xruns;
  $("#st-engine").className = "dot" + (d.running ? " ok" : "");
  $("#st-engine-v").textContent = d.running ? "on" : "off";
  $("#st-scene").textContent = d.scene;
  $("#st-dirty").classList.toggle("hidden", !d.dirty);
  S.dirty = d.dirty;
  const m = $("#master");
  if (document.activeElement !== m) { m.value = d.master_db; $("#master-v").textContent = fmtDb(d.master_db); }
  const rec = $("#btn-rec");
  rec.classList.toggle("on", d.rec.recording);
  $("#rec-label").textContent = d.rec.recording ? fmtTime(d.rec.seconds) : "Rec";
  for (const ev of d.midi) {
    S.midiLog.unshift(ev); S.activity[ev.port] = performance.now();
    if (ev.port !== 8 && ev.port !== 9 && (ev.kind === "note-on" || ev.kind === "note-off")) markKey(ev.bytes[1], ev.kind === "note-on");
  }
  if (S.midiLog.length > 200) S.midiLog.length = 200;
  if (d.midi.length) updateMonitor();
  const lim = $("#lim-v"); if (lim) lim.textContent = d.limiter_db < -0.05 ? d.limiter_db.toFixed(1) + " dB" : "0.0 dB";
  $$(".act[data-port]").forEach((el) => el.classList.toggle("hot", performance.now() - (S.activity[el.dataset.port] || -1e9) < 400));
}
const fmtTime = (s) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;

function onSys() {
  const s = S.sys; if (!s) return;
  $("#st-temp").textContent = s.temp_c != null ? s.temp_c.toFixed(0) + "°C" : "—";
  const dac = s.dac;
  $("#st-dac").className = "dot" + (dac ? (dac.state === "RUNNING" ? " ok" : dac.state === "SUSPENDED" || dac.state === "IDLE" ? "" : " warn") : "");
  $("#st-dac-v").textContent = dac ? dac.state.toLowerCase() : "none";
  if (S.view === "overview") {
    S.sysTick = (S.sysTick || 0) + 1;
    if (S.sysTick % 2 === 0) get("/midi/inputs").then((v) => { S.inputs = v; }).catch(() => {});
    renderOverview();
  }
}

$("#master").addEventListener("input", (e) => { $("#master-v").textContent = fmtDb(+e.target.value); debounce("master", () => post("/master", { db: +e.target.value }).catch(fail), 60); });
$("#btn-panic").addEventListener("click", () => post("/panic").then(() => toast("All sound off")).catch(fail));
$("#btn-rec").addEventListener("click", async () => {
  const on = !(S.status && S.status.rec.recording);
  try { const r = await post("/record", { on }); toast(on ? `Recording ${r.id}` : `Saved ${r.card.id} (${r.card.duration_s.toFixed(1)} s). Stays on this device until filed to the NAS.`); } catch (e) { fail(e); }
});

const timers = {};
function debounce(key, fn, ms) { clearTimeout(timers[key]); timers[key] = setTimeout(fn, ms); }

// ------------------------------------------------------------------ router
$("#nav").addEventListener("click", (e) => { const a = e.target.closest("a[data-view]"); if (a) { location.hash = a.dataset.view; } });
window.addEventListener("hashchange", () => { S.view = location.hash.slice(1) || "overview"; render(); });

function render() {
  $$("#nav a").forEach((a) => a.classList.toggle("on", a.dataset.view === S.view));
  const views = { overview: renderOverview, sounds: renderSounds, midimap: renderMidiMap, scenes: renderScenes, signals: renderSignals, settings: renderSettings };
  (views[S.view] || renderOverview)();
}

// Scene TOML uses [[slot]] / [[route]]; the UI works with plural names.
function norm(sc) { sc.slots = sc.slot || []; sc.routes = sc.route || []; return sc; }

async function refreshScene() {
  try { const r = await get("/scene"); S.scene = norm(r.scene); S.dirty = r.dirty; S.inputs = r.midi_inputs; S.devices = r.devices || []; } catch (e) { fail(e); }
}

function head(title, sub, right = "") {
  return `<div class="page-head"><div><h2>${esc(title)}</h2>${sub ? `<div class="sub">${sub}</div>` : ""}</div><div class="spacer"></div>${right}</div>`;
}

// ------------------------------------------------------------------ overview
function renderOverview() {
  const s = S.sys;
  if (!s) { $("#main").innerHTML = head("Overview") + `<div class="empty-state">Waiting for the device…</div>`; return; }
  const b = s.backend;
  const cores = (s.cores || []).map((c) => `<div class="b"><i style="height:${Math.min(100, c).toFixed(0)}%"></i></div>`).join("");
  const coreLbl = (s.cores || []).map((c, i) => `<span>${i === 3 ? "cpu3·rt" : "cpu" + i} ${c.toFixed(0)}%</span>`).join("");
  const svc = (s.services || []).map((x) => `
    <tr>
      <td><span class="dot ${x.active ? "ok" : ""}" style="display:inline-block;margin-right:8px"></span>${esc(x.name)}</td>
      <td class="muted small">${esc(x.ports)}</td>
      <td><span class="badge ${x.severity}" title="${esc(x.risk)}">${x.severity}</span></td>
      <td style="text-align:right">${x.control === "console"
        ? `<label class="switch" title="${esc(x.risk)}"><input type="checkbox" data-svc="${x.id}" ${x.active ? "checked" : ""}><span></span></label>`
        : x.id === "pilab-console" ? `<span class="muted small" title="${esc(x.how)}">on · access path</span>`
        : `<span class="muted small mono" title="${esc(x.how)}">${x.active ? "on" : "off"}</span>`}</td>
    </tr>`).join("");
  const ports = (s.listening || []).map((p) => `<tr><td class="mono">${p.proto}</td><td class="num">${p.port}</td><td>${p.scope}</td><td>${p.service === "undeclared" ? `<span class="badge caution">undeclared</span>` : esc(p.service)}</td></tr>`).join("");
  const usb = (s.usb || []).map((u) => `<tr><td>${esc(u.product || u.id)}</td><td class="muted">${esc(u.vendor)}</td><td class="mono small">${u.id}</td><td>${u.midi ? `<span class="badge ok">MIDI</span>` : u.audio ? `<span class="badge">audio</span>` : ""}</td></tr>`).join("") || `<tr><td class="muted">No USB devices</td></tr>`;
  const inputs = (S.inputs || []).filter((i) => i.slot !== 9).map((i) => {
    const who = i.device ? `<span class="badge ok">${esc(i.device)}</span> ` : "";
    const ident = i.identity ? `<div class="mono small muted">${esc(i.identity)}${i.rule ? " · matched by " + esc(i.rule) : ""}</div>` : "";
    if (i.slot == null) return `<tr class="muted"><td><span class="act"></span>${who}${esc(i.label)}${ident}<div class="small">${esc(i.skipped)}</div></td><td class="mono small">not connected</td><td class="num">—</td></tr>`;
    return `<tr><td><span class="act" data-port="${i.slot}"></span>${who}${esc(i.label)}${ident}</td><td class="mono small muted">${i.slot === 0 ? "pilab-synth:midi_in" : i.slot === 8 ? "web keyboard" : "in " + i.slot}</td><td class="num">${i.events}</td></tr>`;
  }).join("");
  const ev = (s.events || []).map((e) => `<div><span class="t">${e.time}</span><span>${e.level}</span><span></span><span>${esc(e.text)}</span></div>`).join("");
  const rec = s.recording;
  $("#main").innerHTML = head("Overview", `${esc(s.hostname)}.local · ${esc((s.addresses || []).join(" · "))} · port ${s.port}`) + `
  <div class="grid g4">
    <div class="card">
      <div class="card-head"><h3>Machine</h3></div>
      <div class="stats"><div class="stat"><div class="big">${s.temp_c != null ? s.temp_c.toFixed(1) : "—"}<small>°C</small></div><div class="lbl">SoC</div></div>
      <div class="stat"><div class="big">${s.mem_pct.toFixed(0)}<small>%</small></div><div class="lbl">Memory</div></div></div>
      <div class="bars">${cores}</div><div class="bars-lbl">${coreLbl}</div>
      <div class="kv" style="margin-top:10px"><span class="k">Load</span><span class="v">${esc(s.load)}</span><span class="k">Uptime</span><span class="v">${fmtUptime(s.uptime_s)}</span></div>
    </div>
    <div class="card">
      <div class="card-head"><h3>Audio</h3></div>
      <div class="stats"><div class="stat"><div class="big">${(b.latency_ms || 0).toFixed(2)}<small>ms</small></div><div class="lbl">Period</div></div>
      <div class="stat"><div class="big">${(b.sample_rate / 1000).toFixed(0)}<small>kHz</small></div><div class="lbl">Rate</div></div></div>
      <div class="kv">
        <span class="k">Backend</span><span class="v">${esc(b.name)}</span>
        <span class="k">Buffer</span><span class="v">${b.buffer} frames</span>
        <span class="k">Audio thread</span><span class="v">${b.pinned === 1 ? "CPU 3" : b.pinned === 0 ? "not started" : "unpinned"} · ${b.policy}${b.rt_prio ? " " + b.rt_prio : ""}</span>
        <span class="k">DAC sink</span><span class="v">${s.dac ? esc(s.dac.state.toLowerCase() + " · " + s.dac.format) : "not found"}</span>
        <span class="k">Limiter</span><span class="v" id="lim-v">—</span>
        <span class="k">Watchdog</span><span class="v">${s.watchdog.enabled ? "on" : "off"}${s.watchdog.last_action ? " · last " + esc(s.watchdog.last_action.slice(11, 19)) : ""}</span>
      </div>
    </div>
    <div class="card span2">
      <div class="card-head"><h3>Services</h3><span class="spacer"></span><span class="muted small">Risk notes show on hover and when switched</span></div>
      <table><tbody>${svc}</tbody></table>
    </div>
    <div class="card">
      <div class="card-head"><h3>MIDI inputs</h3></div>
      <table><tbody>${inputs}</tbody></table>
    </div>
    <div class="card">
      <div class="card-head"><h3>Recording</h3><span class="spacer"></span><a class="muted small" href="#settings">Settings</a></div>
      <div class="kv">
        <span class="k">Local</span><span class="v">${rec.enabled ? "allowed" : "off"}</span>
        <span class="k">Free</span><span class="v">${rec.free_mb != null ? (rec.free_mb / 1024).toFixed(1) + " GB" : "—"}</span>
        <span class="k">Not filed</span><span class="v">${rec.unfiled}${rec.oldest_unfiled_hours != null ? " · oldest " + rec.oldest_unfiled_hours.toFixed(0) + " h" : ""}</span>
        <span class="k">NAS archive</span><span class="v">${esc(s.archive.state)}</span>
      </div>
      ${rec.warn ? `<div class="risk caution" style="margin-top:10px"><b>!</b> ${rec.unfiled} recording(s) exist only on this device. File them to the NAS Archive.</div>` : ""}
    </div>
    <div class="card span2">
      <div class="card-head"><h3>Listening ports</h3><span class="spacer"></span><span class="muted small">From the kernel; anything undeclared is flagged</span></div>
      <div class="tbl-scroll"><table><thead><tr><th>Proto</th><th class="num">Port</th><th>Scope</th><th>Owner</th></tr></thead><tbody>${ports}</tbody></table></div>
    </div>
    <div class="card span2">
      <div class="card-head"><h3>USB devices</h3></div>
      <table><tbody>${usb}</tbody></table>
    </div>
    <div class="card span2">
      <div class="card-head"><h3>Version</h3></div>
      <div id="ov-versions"><div class="muted small">Loading…</div></div>
    </div>
    <div class="card span2">
      <div class="card-head"><h3>Storage</h3><span class="spacer"></span><span class="muted small">lsblk · temperature where the drive reports it</span></div>
      <div id="ov-storage" class="tbl-scroll"><div class="muted small">Loading…</div></div>
    </div>
    <div class="card span2">
      <div class="card-head"><h3>Events</h3></div>
      <div class="monitor">${ev}</div>
    </div>
  </div>`;
  $$("[data-svc]").forEach((el) => el.addEventListener("change", async (e) => {
    const on = e.target.checked;
    const x = s.services.find((v) => v.id === e.target.dataset.svc);
    let confirm = "";
    if (x.kind === "access" && !on) {
      confirm = await dialog({
        title: `Turn off ${x.name}?`,
        body: `<p>${esc(x.name)} is a remote access path. The console stays on, so you can still get in from this page, but if the console stops too, the only way back in is a local monitor and keyboard, or editing the SD card.</p><p>Type <b class="mono">turn off ssh</b> to confirm.</p>`,
        typed: "turn off ssh", ok: "Turn off",
      });
      if (confirm == null) { e.target.checked = !on; return; }
    } else if (on && x.id !== "pilab-synth") {
      const go = await dialog({ title: `Turn on ${x.name}?`, body: `<p>${esc(x.risk)}</p><p class="muted small">Ports: ${esc(x.ports)}</p>`, ok: "Turn on" });
      if (go == null) { e.target.checked = !on; return; }
    }
    e.target.disabled = true;
    try {
      const r = await post(`/services/${x.id}`, { on, confirm });
      toast((on ? "On. " : "Off. ") + (r.message || x.risk));
    } catch (err) { fail(err); e.target.checked = !on; }
    e.target.disabled = false;
  }));
  loadOverviewExtras();
}

// Version and storage panels load once per visit (slow-changing, a few subprocesses).
async function loadOverviewExtras() {
  try {
    if (!S.ovx || Date.now() - S.ovx.t > 60000) {
      if (S.ovxBusy) return; S.ovxBusy = true;
      try { const [v, st] = await Promise.all([get("/versions"), get("/storage")]); S.ovx = { t: Date.now(), v, st }; } finally { S.ovxBusy = false; }
    }
    const { v, st } = S.ovx;
    const vEl = $("#ov-versions"), sEl = $("#ov-storage");
    if (vEl) vEl.innerHTML = `<div class="kv">
      <span class="k">Console</span><span class="v">${esc(v.console.name)} ${esc(v.console.version)} · <span class="mono">${esc(v.console.commit)}</span></span>
      <span class="k">Model</span><span class="v">${esc(v.model || "—")}</span>
      <span class="k">OS</span><span class="v">${esc(v.os || "—")}</span>
      <span class="k">Kernel</span><span class="v mono small">${esc(v.kernel || "—")}</span>
      <span class="k">PipeWire</span><span class="v">${esc(v.pipewire || "—")}</span>
      <span class="k">Firmware</span><span class="v small">${esc(v.firmware || "—")}</span>
      <span class="k">Bootloader</span><span class="v small">${esc(v.bootloader || "—")}</span>
      <span class="k">Device id</span><span class="v mono small">${esc(v.device_id)}</span></div>`;
    const gib = (b) => b == null ? "—" : b >= 1e12 ? (b / 1e12).toFixed(2) + " TB" : (b / 1e9).toFixed(1) + " GB";
    const rows = [];
    for (const d of st.blockdevices || []) {
      rows.push(`<tr><td><b>${esc(d.kind)}</b> <span class="mono small muted">${esc(d.name)}</span>${d.model ? `<div class="small muted">${esc(d.model)}</div>` : ""}</td><td class="num">${gib(+d.size)}</td><td colspan="4"></td><td class="num">${d.temp_c != null ? d.temp_c.toFixed(0) + " °C" : ""}</td></tr>`);
      for (const p of (d.children || []).concat(d.mountpoint ? [d] : [])) {
        const used = p.fsused != null ? +p.fsused : null, size = p.fssize != null ? +p.fssize : null;
        const pct = used != null && size ? used / size * 100 : null;
        rows.push(`<tr class="sub"><td class="mono small">${esc(p.name)}${p.label ? ` <span class="muted">${esc(p.label)}</span>` : ""}</td><td class="num">${gib(+p.size)}</td>
          <td class="num">${gib(used)}</td><td class="num">${gib(p.fsavail != null ? +p.fsavail : null)}</td>
          <td>${pct != null ? `<div class="meter-bar ${pct > 90 ? "hot" : ""}"><i style="width:${pct.toFixed(0)}%"></i></div><span class="small">${pct.toFixed(0)}%</span>` : `<span class="muted small">not mounted</span>`}</td>
          <td class="small">${esc(p.fstype || "")} ${p.mountpoint ? `<span class="mono muted">${esc(p.mountpoint)}</span>` : ""}</td><td></td></tr>`);
      }
    }
    if (sEl) sEl.innerHTML = `<table class="storage"><thead><tr><th>Device</th><th class="num">Size</th><th class="num">Used</th><th class="num">Free</th><th>Use</th><th>Filesystem · mount</th><th class="num">Temp</th></tr></thead><tbody>${rows.join("") || `<tr><td class="muted">No disks found</td></tr>`}</tbody></table>`;
  } catch (e) { /* panels stay as placeholders */ }
}

// Modal dialog. Resolves to the typed text (or "") on OK, null on cancel.
function dialog({ title, body, typed = "", ok = "OK" }) {
  return new Promise((resolve) => {
    const d = document.createElement("dialog");
    d.className = "dlg";
    d.innerHTML = `<h3>${esc(title)}</h3><div class="dlg-body">${body}</div>${typed ? `<input class="input mono" id="dlg-typed" autocomplete="off" placeholder="${esc(typed)}">` : ""}
      <div class="row" style="justify-content:flex-end;margin-top:14px"><button class="btn ghost" data-x="cancel">Cancel</button><button class="btn" data-x="ok" ${typed ? "disabled" : ""}>${esc(ok)}</button></div>`;
    document.body.appendChild(d);
    const okb = $('[data-x="ok"]', d), inp = $("#dlg-typed", d);
    if (inp) inp.addEventListener("input", () => { okb.disabled = inp.value.trim().toLowerCase() !== typed; });
    const done = (v) => { d.close(); d.remove(); resolve(v); };
    okb.addEventListener("click", () => done(inp ? inp.value.trim() : ""));
    $('[data-x="cancel"]', d).addEventListener("click", () => done(null));
    d.addEventListener("cancel", () => done(null));
    d.showModal();
    (inp || okb).focus();
  });
}
const fmtUptime = (s) => s < 3600 ? `${Math.floor(s / 60)} min` : `${Math.floor(s / 3600)} h ${Math.floor(s % 3600 / 60)} min`;

// ------------------------------------------------------------------ sounds
function slotById(id) { return S.scene.slots.find((s) => s.id === id); }
function defaultSlot(id) { return { id, name: "Init " + id, table: "", sound: { enabled: true, wave: "saw", pulse_width: 0.5, pwm_mod: 0.3, octave: 0, detune_cents: 0, level_db: -6, pan: 0, vel_to_amp: 0.8, amp_env: { a: 0.01, d: 0.3, s: 0.7, r: 0.4 }, filter: { enabled: true, cutoff_hz: 4000, resonance: 0.15, env_amount: 1.5, key_track: 0.5, vel_amount: 1.0 }, filter_env: { a: 0.005, d: 0.4, s: 0.2, r: 0.4 } } }; }

const P = (key, label, min, max, step, fmt, scale) => ({ key, label, min, max, step, fmt, scale });
const OSC_PARAMS = [P("octave", "Octave", -3, 3, 1, (v) => (v > 0 ? "+" : "") + v), P("detune_cents", "Detune", -100, 100, 1, (v) => v + " ct"), P("level_db", "Level", -40, 0, 0.5, (v) => v.toFixed(1) + " dB"), P("pan", "Pan", -1, 1, 0.01, (v) => v === 0 ? "C" : (v < 0 ? "L" : "R") + Math.abs(v * 100).toFixed(0)), P("vel_to_amp", "Velocity", 0, 1, 0.01, (v) => (v * 100).toFixed(0) + "%")];
const PW_PARAMS = [P("pulse_width", "Pulse width", 0.02, 0.98, 0.01, (v) => (v * 100).toFixed(0) + "%"), P("pwm_mod", "Mod wheel → PW", 0, 0.5, 0.01, (v) => (v * 100).toFixed(0) + "%")];
const ENV_PARAMS = [P("a", "Attack", 0.0005, 10, 0, fmtSec, "log"), P("d", "Decay", 0.001, 10, 0, fmtSec, "log"), P("s", "Sustain", 0, 1, 0.01, (v) => (v * 100).toFixed(0) + "%"), P("r", "Release", 0.003, 15, 0, fmtSec, "log")];
const FLT_PARAMS = [P("cutoff_hz", "Cutoff", 20, 20000, 0, (v) => v >= 1000 ? (v / 1000).toFixed(2) + " kHz" : v.toFixed(0) + " Hz", "log"), P("resonance", "Resonance", 0, 0.97, 0.01, (v) => v.toFixed(2)), P("env_amount", "Env → cutoff", -4, 6, 0.1, (v) => (v > 0 ? "+" : "") + v.toFixed(1) + " oct"), P("key_track", "Key track", 0, 1, 0.01, (v) => (v * 100).toFixed(0) + "%"), P("vel_amount", "Vel → cutoff", 0, 4, 0.1, (v) => v.toFixed(1) + " oct")];
const pct = (v) => (v * 100).toFixed(0) + "%";
const spct = (v) => (v > 0 ? "+" : "") + (v * 100).toFixed(0) + "%";
const WT_PARAMS = [P("position", "Position", 0, 1, 0.001, pct)];
const MOD_PARAMS = [P("pos_env", "Mod env → position", -1, 1, 0.01, spct), P("pos_lfo", "LFO → position", 0, 1, 0.01, pct), P("lfo_hz", "LFO rate", 0.05, 20, 0, (v) => v.toFixed(2) + " Hz", "log"), P("pos_mod", "Mod wheel → position", -1, 1, 0.01, spct)];
const WARP_PARAMS = [P("warp_amount", "Warp amount", -1, 1, 0.01, spct)];
const UNI_PARAMS = [P("unison", "Unison", 1, 3, 1, (v) => v === 1 ? "off" : v + " voices"), P("unison_detune", "Detune spread", 0, 50, 0.5, (v) => v.toFixed(1) + " ct"), P("drive", "Drive", 0, 1, 0.01, pct)];
const WARPS = ["none", "bend", "sync", "mirror", "asym", "fold"];
const ALL_SOUND_PARAMS = () => OSC_PARAMS.concat(PW_PARAMS, WT_PARAMS, MOD_PARAMS, WARP_PARAMS, UNI_PARAMS);
// Human names for AKWF folders come from data/akwf-catalog.json (edit that file, not code).
function wtInfo(group) {
  const c = S.cat && S.cat.groups[group];
  return c || { name: String(group || "").replace(/^AKWF_/, "").replace(/_/g, " "), tags: [], about: "" };
}
const waveNum = (f) => { const m = String(f).match(/(\d+)\.wav$/i); return m ? "#" + (+m[1]) : String(f).split("/").pop(); };
function isFav(k) { return S.fav.includes(k); }
function toggleFav(k) { S.fav = isFav(k) ? S.fav.filter((x) => x !== k) : S.fav.concat([k]); localStorage.setItem("pilab.fav", JSON.stringify(S.fav)); }
function fmtSec(v) { return v < 1 ? (v * 1000).toFixed(v < 0.01 ? 1 : 0) + " ms" : v.toFixed(2) + " s"; }
const toPos = (p, v) => p.scale === "log" ? Math.log(v / p.min) / Math.log(p.max / p.min) * 1000 : v;
const fromPos = (p, x) => p.scale === "log" ? p.min * Math.pow(p.max / p.min, x / 1000) : +x;

function paramHtml(group, p, v) {
  const min = p.scale === "log" ? 0 : p.min, max = p.scale === "log" ? 1000 : p.max, step = p.scale === "log" ? 1 : p.step;
  return `<div class="param"><div class="top-line"><span class="lbl">${p.label}</span><span class="val" data-val="${group}.${p.key}">${p.fmt(v)}</span></div>
    <input type="range" min="${min}" max="${max}" step="${step}" value="${toPos(p, v)}" data-p="${group}.${p.key}"></div>`;
}

function renderSounds() {
  if (!S.scene) return;
  const ids = Array.from({ length: 16 }, (_, i) => i + 1);
  const list = ids.map((id) => {
    const s = slotById(id);
    return `<div class="slot ${id === S.selSlot ? "on" : ""} ${s ? "" : "empty"}" data-slot="${id}"><span class="num">${id}</span><span class="nm">${s ? esc(s.name) : "empty"}</span>${s ? `<span class="badge mono" title="${s.sound.wave === "table" && s.wavetable ? esc(wtInfo(s.wavetable).name) : ""}">${WAVE_LABEL[s.sound.wave] || s.sound.wave}</span>` : ""}</div>`;
  }).join("");
  const s = slotById(S.selSlot);
  let editor;
  if (!s) {
    editor = `<div class="card"><div class="empty-state">Slot ${S.selSlot} is empty.<br><br><button class="btn ghost" id="slot-new">Create sound in slot ${S.selSlot}</button></div></div>`;
  } else {
    const snd = s.sound;
    const used = S.scene.routes.filter((r) => r.slot === s.id && r.enabled).length;
    editor = `<div class="card" id="slot-editor">
      <div class="card-head"><input class="input" id="slot-name" value="${esc(s.name)}" title="${s.name_locked ? "Name set by hand" : "Named after its sound; type to set your own"}" style="font-family:var(--font-display);font-size:18px;width:260px">${s.name_locked ? `<button class="btn ghost small" id="slot-autoname" title="Name it after its sound again">Auto name</button>` : ""}
        <span class="spacer"></span>
        <label class="row tight muted small"><span>Enabled</span><label class="switch"><input type="checkbox" id="slot-en" ${snd.enabled ? "checked" : ""}><span></span></label></label>
        <button class="btn ghost small" data-play="slot:${s.id}" title="Play a short phrase">▶ Play</button>
        <button class="btn ghost small" id="slot-del">Remove</button>
      </div>
      <div class="row" style="margin-bottom:10px"><div class="seg" id="wave-seg">${WAVES.map((w) => `<button data-w="${w}" class="${snd.wave === w ? "on" : ""}">${WAVE_LABEL[w] || w}</button>`).join("")}</div>
        <span class="muted small">${snd.wave === "table" ? (s.wavetable ? `Wavetable: <b>${esc(wtInfo(s.wavetable).name)}</b> <span class="mono">${S.akwfFrames && S.akwfFrames.group === s.wavetable ? "· wave " + (Math.round(snd.position * (S.akwfFrames.frames.length - 1)) + 1) + " of " + S.akwfFrames.frames.length : ""}</span>` : s.table ? "Table: <span class='mono'>" + esc(s.table) + "</span>" : "Pick a wavetable from the browser →") : snd.wave === "sampler" ? (s.sample ? "Samples: <b>" + esc(bankTitle(s.sample)) + "</b>" : "Pick a sample bank or preset from the browser →") : ""}</span></div>
      <canvas class="wave" id="wave-cv" width="900" height="240"></canvas>
      <div class="params" style="margin-top:12px">${OSC_PARAMS.map((p) => paramHtml("sound", p, snd[p.key])).join("")}${snd.wave === "pulse" ? PW_PARAMS.map((p) => paramHtml("sound", p, snd[p.key])).join("") : ""}</div>
      ${snd.wave !== "sampler" ? `<div class="card deep" style="margin-top:14px"><div class="card-head"><h3>${snd.wave === "table" ? "Wavetable" : "Shape"}</h3><span class="spacer"></span>
          <div class="seg" id="warp-seg">${WARPS.map((w) => `<button data-warp="${w}" class="${(snd.warp || "none") === w ? "on" : ""}">${w}</button>`).join("")}</div></div>
        ${snd.wave === "table" && s.wavetable ? `<canvas class="stack" id="wt-stack" width="900" height="150"></canvas>` : ""}
        <div class="params">${snd.wave === "table" && s.wavetable ? WT_PARAMS.map((p) => paramHtml("sound", p, snd[p.key] ?? 0)).join("") : ""}${(snd.warp || "none") !== "none" ? WARP_PARAMS.map((p) => paramHtml("sound", p, snd[p.key] ?? 0)).join("") : ""}${UNI_PARAMS.map((p) => paramHtml("sound", p, snd[p.key] ?? (p.key === "unison" ? 1 : 0))).join("")}</div>
        ${snd.wave === "table" && s.wavetable ? `<div class="lbl" style="margin:10px 0 4px">Position modulation</div><div class="params">${MOD_PARAMS.map((p) => paramHtml("sound", p, snd[p.key] ?? (p.key === "lfo_hz" ? 0.5 : 0))).join("")}</div><div class="muted small">The mod envelope is the filter envelope; the LFO is a free-running sine per voice; the mod wheel is CC 1.</div>` : ""}
      </div>` : ""}
      <div class="grid g2" style="margin-top:14px">
        <div class="card deep"><div class="card-head"><h3>Amp envelope</h3></div><canvas class="env" id="env-amp" width="600" height="128"></canvas><div class="params" style="margin-top:8px">${ENV_PARAMS.map((p) => paramHtml("amp_env", p, snd.amp_env[p.key])).join("")}</div></div>
        <div class="card deep"><div class="card-head"><h3>Filter</h3><span class="spacer"></span><label class="switch"><input type="checkbox" id="flt-en" ${snd.filter.enabled ? "checked" : ""}><span></span></label></div>
          <div class="params">${FLT_PARAMS.map((p) => paramHtml("filter", p, snd.filter[p.key])).join("")}</div>
          <div class="lbl" style="margin:10px 0 4px">Filter envelope</div><div class="params">${ENV_PARAMS.map((p) => paramHtml("filter_env", p, snd.filter_env[p.key])).join("")}</div></div>
      </div>
      <div class="muted small" style="margin-top:10px">${used ? `Played by ${used} route(s) in the MIDI map.` : `No route plays this sound yet. Add one in <a href="#midimap">MIDI Map</a>, or use the keyboard in “Selected sound” mode.`}</div>
    </div>`;
  }
  const ak = S.akwf;
  let akHtml;
  if (!ak) akHtml = `<div class="muted">Loading…</div>`;
  else if (!ak.available) akHtml = `<div class="risk">AKWF library not found at <code>${esc(ak.root)}</code>. Run <code>scripts/fetch-akwf.sh</code> on the Pi.</div>`;
  else {
    const q = S.wtQuery.toLowerCase();
    const groups = ak.groups.map((g) => ({ ...g, info: wtInfo(g.name) }))
      .filter((g) => (!S.wtTag || (S.wtTag === "★" ? isFav(g.name) || S.fav.some((f) => f.startsWith(g.name + "/")) : g.info.tags.includes(S.wtTag))) && (!q || (g.info.name + " " + g.name + " " + g.info.tags.join(" ")).toLowerCase().includes(q)))
      .sort((x, y) => (isFav(y.name) - isFav(x.name)) || x.info.name.localeCompare(y.info.name));
    const tags = ["★"].concat((S.cat && S.cat.tags) || []);
    const fr = S.akwfFrames && S.akwfFrames.group === S.akwfGroup ? S.akwfFrames.frames : null;
    const inSlot = s && s.sound.wave === "table" && s.wavetable === S.akwfGroup;
    const cur = inSlot && fr ? Math.round((s.sound.position || 0) * (fr.length - 1)) : -1;
    akHtml = `<input class="input" id="wt-q" placeholder="Search names or tags" value="${esc(S.wtQuery)}" style="width:100%;margin-bottom:6px">
      <div class="tagrow">${tags.map((t) => `<button class="chip ${S.wtTag === t ? "on" : ""}" data-tag="${esc(t)}">${t === "★" ? "★ Favorites" : esc(t)}</button>`).join("")}</div>
      <div class="akwf-groups">${groups.map((g) => `<div data-g="${esc(g.name)}" class="${g.name === S.akwfGroup ? "sel" : ""}" title="${esc(g.info.about)}"><span><span class="fav ${isFav(g.name) ? "on" : ""}" data-fav="${esc(g.name)}">★</span>${esc(g.info.name)}<span class="muted small"> · ${esc(g.info.tags.join(", "))}</span></span><span class="mono muted small">${g.count}</span></div>`).join("") || `<div class="muted small">No match</div>`}</div>
      ${S.akwfGroup ? `<div class="card-head" style="margin-top:6px"><h3 style="text-transform:none">${esc(wtInfo(S.akwfGroup).name)}</h3><span class="spacer"></span><span class="mono muted small">${esc(S.akwfGroup)}</span></div>
        <div class="muted small" style="margin-bottom:6px">${esc(wtInfo(S.akwfGroup).about)} Click a wave to use the whole group as a wavetable at that position.</div>
        <canvas class="scope" id="wt-scope" width="560" height="120"></canvas>
        <div class="wt-grid">${fr ? fr.map((f, i) => `<div class="wt ${i === cur ? "sel" : ""}" data-wi="${i}" data-f="${esc(f.file)}" title="${esc(f.file.split("/").pop())}"><canvas data-thumb="${i}" width="96" height="40"></canvas><span class="mono small">${waveNum(f.file)}</span><span class="fav ${isFav(f.file) ? "on" : ""}" data-fav="${esc(f.file)}">★</span>${playBtn(`data-play="table:${esc(f.file)}"`)}</div>`).join("") : `<div class="muted small">Loading waves…</div>`}</div>` : `<div class="muted small">Pick a group to see its waves.</div>`}`;
  }
  const sm = S.samples, pr = S.presets;
  let smHtml;
  if (!sm || !pr) smHtml = `<div class="muted">Loading…</div>`;
  else if (!sm.available) smHtml = `<div class="risk">No sample banks installed at <code>${esc(sm.root)}</code>. On the Pi run <code>sh scripts/fetch-samples.sh</code> (CC0 FreePats banks and the VCSL drum kit, about 120 MiB).</div>`;
  else {
    const cats = {};
    sm.banks.forEach((b) => (cats[b.category] = cats[b.category] || []).push(b));
    const CAT = { keys: "Keys", "strings-and-plucked": "Strings and plucked", winds: "Winds", "mallets-and-bells": "Mallets and bells", drums: "Drums and percussion", synths: "Synths" };
    smHtml = `<div class="lbl" style="margin-bottom:6px">Presets</div>
      <div class="presets">${pr.presets.filter((p) => p.installed).map((p) => `<div class="preset" data-preset="${p.id}" title="Use in slot ${S.selSlot}"><span>${esc(p.name)}</span>${p.drum ? `<span class="badge">ch 10</span>` : ""}${playBtn(`data-play="preset:${p.id}"`)}</div>`).join("")}</div>
      <div class="banks">${Object.keys(CAT).filter((c) => cats[c]).map((c) => `<div class="lbl" style="margin:10px 0 4px">${CAT[c]}</div>` + cats[c].map((b) => `<div class="bank ${s && s.sample === b.id ? "sel" : ""}" data-bank="${esc(b.id)}" title="Use in slot ${S.selSlot}"><span>${esc(b.title)}</span><span class="mono muted small">${(b.bytes / 1048576).toFixed(1)} MiB</span>${playBtn(`data-play="bank:${esc(b.id)}"`)}</div>`).join("")).join("")}</div>
      <div class="muted small" style="margin-top:8px">FreePats and VCSL, CC0 1.0. Banks load into RAM on first use${sm.loaded_mib ? ` (${sm.loaded_mib.toFixed(0)} MiB loaded)` : ""}.</div>`;
  }
  const tab = S.browserTab;
  $("#main").innerHTML = head("Sounds", `Sound slots for scene <b>${esc(S.scene.name)}</b>. Edits play live; save the scene in Scenes.`, `${outputSeg()}<button class="btn primary" id="save-now">Save scene</button>`) + `
  <div class="sounds ${tab === "akwf" ? "wide" : ""}">
    <div class="card"><div class="card-head"><h3>Slots</h3></div><div class="slotlist">${list}</div></div>
    ${editor}
    <div class="card"><div class="card-head"><div class="seg" id="tab-seg"><button data-t="samples" class="${tab === "samples" ? "on" : ""}">Samples</button><button data-t="akwf" class="${tab === "akwf" ? "on" : ""}">Wavetables</button></div><span class="spacer"></span><span class="badge">CC0</span></div>
      ${tab === "samples" ? smHtml : akHtml + `<div class="muted small" style="margin-top:8px">Adventure Kid Waveforms, single-cycle, CC0 1.0. Mipmapped on load.</div>`}</div>
  </div>`;
  bindSounds();
  if (s) { drawWave(); drawEnv("env-amp", s.sound.amp_env); }
  if (!S.akwf) get("/library/akwf").then((r) => { S.akwf = r; if (S.view === "sounds") renderSounds(); }).catch(fail);
  if (!S.samples) Promise.all([get("/library/samples"), get("/library/presets")]).then(([a, b]) => { S.samples = a; S.presets = b; if (S.view === "sounds") renderSounds(); }).catch(fail);
  if (!S.cat) fetch("/data/akwf-catalog.json").then((r) => r.json()).then((c) => { S.cat = c; if (S.view === "sounds") renderSounds(); }).catch(() => { S.cat = { groups: {}, tags: [] }; });
  if (!S.akwfGroup && s && s.wavetable) S.akwfGroup = s.wavetable;
  if (S.akwfGroup && (!S.akwfFrames || S.akwfFrames.group !== S.akwfGroup)) loadFrames(S.akwfGroup);
  drawWtViews(s);
  post("/audition", { slot: s ? s.id : 0 }).catch(() => {});
}

function bankTitle(id) { const b = S.samples && S.samples.banks.find((x) => x.id === id); return b ? b.title : id; }

async function applyPreset(body) {
  if (!S.selSlot) return toast("Select a sound slot first.");
  const id = S.selSlot;
  clearTimeout(timers.slot);
  setLoading(true, "Loading " + (body.preset ? body.preset : bankTitle(body.bank)) + "…");
  const job = pushChain.then(async () => {
    try {
      const r = await post(`/scene/slots/${id}/preset`, body);
      S.scene = norm(r.scene); S.dirty = r.dirty;
      setLoading(false);
      renderSounds();
      const s = slotById(id);
      if (s && s.sound.wave === "sampler") { if (S.prevSel) audition({ slot: id }); else toast(`${s.name} ready`); }
    } catch (e) { setLoading(false); fail(e); }
  });
  pushChain = job.catch(() => {});
  return job;
}
function setLoading(on, msg) {
  const ed = $("#slot-editor"); if (ed) ed.classList.toggle("loading", on);
  if (on && msg) toast(msg);
}

function bindSounds() {
  bindOutput(renderSounds);
  bindPlay($("#main"));
  $$("#tab-seg button").forEach((b) => b.addEventListener("click", () => { S.browserTab = b.dataset.t; localStorage.setItem("pilab.tab", S.browserTab); renderSounds(); }));
  $$("[data-preset]").forEach((el) => el.addEventListener("click", () => applyPreset({ preset: el.dataset.preset })));
  $$("[data-bank]").forEach((el) => el.addEventListener("click", () => applyPreset({ bank: el.dataset.bank })));
  $$(".slot").forEach((el) => el.addEventListener("click", () => {
    S.selSlot = +el.dataset.slot; renderSounds();
    if (S.prevSel && slotById(S.selSlot)) audition({ slot: S.selSlot });
  }));
  $("#save-now").addEventListener("click", () => saveScene(S.scene.name));
  const nw = $("#slot-new");
  if (nw) nw.addEventListener("click", () => pushSlot(defaultSlot(S.selSlot), true));
  const s = slotById(S.selSlot);
  if (!s) return bindAkwf(null);
  $("#slot-name").addEventListener("change", (e) => { s.name = e.target.value.trim(); s.name_locked = s.name !== ""; pushSlot(s, true); });
  const an = $("#slot-autoname"); if (an) an.addEventListener("click", () => { s.name = ""; s.name_locked = false; pushSlot(s, true); });
  $("#slot-en").addEventListener("change", (e) => { s.sound.enabled = e.target.checked; pushSlot(s); });
  $("#flt-en").addEventListener("change", (e) => { s.sound.filter.enabled = e.target.checked; pushSlot(s); });
  $("#slot-del").addEventListener("click", async () => { try { const r = await api("DELETE", `/scene/slots/${s.id}`); S.scene = norm(r.scene); renderSounds(); } catch (e) { fail(e); } });
  $$("#wave-seg button").forEach((b) => b.addEventListener("click", () => {
    const w = b.dataset.w;
    s.sound.wave = w;
    if (w === "table") { S.browserTab = "akwf"; if (!s.wavetable && S.akwfGroup) { s.wavetable = S.akwfGroup; s.table = ""; } }
    if (w === "sampler") S.browserTab = "samples";
    selectApply(s, w === "sampler" ? "Loading samples…" : "");
  }));
  $$("#warp-seg button").forEach((b) => b.addEventListener("click", () => { s.sound.warp = b.dataset.warp; if (s.sound.warp !== "none" && !s.sound.warp_amount) s.sound.warp_amount = 0.3; pushSlot(s, true); }));
  $$("input[data-p]").forEach((inp) => inp.addEventListener("input", () => {
    const [g, k] = inp.dataset.p.split(".");
    const list = g === "sound" ? ALL_SOUND_PARAMS() : g === "filter" ? FLT_PARAMS : ENV_PARAMS;
    const p = list.find((x) => x.key === k);
    const v = fromPos(p, +inp.value);
    const obj = g === "sound" ? s.sound : s.sound[g];
    obj[k] = v;
    $(`[data-val="${g}.${k}"]`).textContent = p.fmt(v);
    if (g === "amp_env") drawEnv("env-amp", s.sound.amp_env);
    if (g === "sound" && (k === "pulse_width")) drawWave();
    if (g === "sound" && k === "unison") obj[k] = Math.round(v);
    if (g === "sound" && k === "position") { drawWtViews(s); }
    debounce("slot", () => pushSlot(s), 80);
  }));
  bindAkwf(s);
}

async function loadFrames(group) {
  if (S.framesBusy === group) return; S.framesBusy = group;
  try { const r = await get(`/library/akwf/${encodeURIComponent(group)}/frames?points=64`); S.akwfFrames = { group, frames: r.frames }; } catch (e) { fail(e); }
  S.framesBusy = null;
  if (S.view === "sounds") renderSounds();
}

function drawTrace(cv, pts, color, lw = 1.5) {
  const c = cvCtx(cv), w = cv.width, h = cv.height;
  const peak = Math.max(0.001, ...pts.map(Math.abs));
  c.strokeStyle = color; c.lineWidth = lw; c.lineJoin = "round"; c.beginPath();
  pts.forEach((v, i) => { const x = i / (pts.length - 1) * w, y = h / 2 - v / peak * h * 0.42; i ? c.lineTo(x, y) : c.moveTo(x, y); });
  c.stroke();
}

// Thumbnails, the large scope, and a stacked (pseudo-3D) view of the whole table.
function drawWtViews(s) {
  const fr = S.akwfFrames && S.akwfFrames.group === S.akwfGroup ? S.akwfFrames.frames : null;
  if (fr) $$("canvas[data-thumb]").forEach((cv) => drawTrace(cv, fr[+cv.dataset.thumb].points, css("--text-2")));
  const scope = $("#wt-scope");
  if (scope && fr) {
    const inSlot = s && s.wavetable === S.akwfGroup;
    const i = S.hoverWave != null ? S.hoverWave : inSlot ? Math.round((s.sound.position || 0) * (fr.length - 1)) : 0;
    drawTrace(scope, fr[Math.min(i, fr.length - 1)].points, css("--text"), 2.5);
  }
  const st = $("#wt-stack");
  if (st && s && S.akwfFrames && S.akwfFrames.group === s.wavetable) {
    const f2 = S.akwfFrames.frames, c = cvCtx(st), w = st.width, h = st.height, n = f2.length;
    const cur = (s.sound.position || 0) * (n - 1);
    const dx = Math.min(4, 160 / n), dy = Math.min(2.2, 70 / n), ww = w - 40 - dx * n, hh = h * 0.42;
    const step = Math.max(1, Math.ceil(n / 40)), ci = Math.round(cur);
    for (let k = n - 1; k >= 0; k--) {
      if (k % step && k !== ci) continue;
      const ox = 20 + k * dx, oy = h - 30 - k * dy, pts = f2[k].points, peak = Math.max(0.001, ...pts.map(Math.abs));
      const on = Math.abs(k - cur) < 0.5;
      c.strokeStyle = on ? css("--text") : css("--line"); c.lineWidth = on ? 2.5 : 1; c.beginPath();
      pts.forEach((v, i) => { const x = ox + i / (pts.length - 1) * ww, y = oy - (v / peak) * hh * 0.5 - hh * 0.5; i ? c.lineTo(x, y) : c.moveTo(x, y); });
      c.stroke();
    }
  }
}

function bindAkwf(s) {
  const q = $("#wt-q");
  if (q) q.addEventListener("input", () => { S.wtQuery = q.value; debounce("wtq", () => { renderSounds(); const e = $("#wt-q"); if (e) { e.focus(); e.setSelectionRange(e.value.length, e.value.length); } }, 200); });
  $$("[data-tag]").forEach((b) => b.addEventListener("click", () => { S.wtTag = S.wtTag === b.dataset.tag ? "" : b.dataset.tag; renderSounds(); }));
  $$("[data-fav]").forEach((el) => el.addEventListener("click", (e) => { e.stopPropagation(); toggleFav(el.dataset.fav); renderSounds(); }));
  // Picking a group plays it: the slot becomes a wavetable on that group, at its first
  // audible wave (pilabd picks the wave and position when table is empty).
  $$(".akwf-groups [data-g]").forEach((el) => el.addEventListener("click", () => {
    S.akwfGroup = el.dataset.g; S.hoverWave = null;
    if (!s) { if (!S.selSlot) return renderSounds(); s = defaultSlot(S.selSlot); }
    if (s.sound.wave === "table" && s.wavetable === S.akwfGroup) return renderSounds();
    s.sound.wave = "table"; s.wavetable = S.akwfGroup; s.table = "";
    selectApply(s);
  }));
  $$(".wt-grid [data-wi]").forEach((el) => {
    el.addEventListener("mouseenter", () => { S.hoverWave = +el.dataset.wi; drawWtViews(s); });
    el.addEventListener("mouseleave", () => { S.hoverWave = null; drawWtViews(s); });
    el.addEventListener("click", () => {
      if (!s) { if (!S.selSlot) return toast("Select a sound slot first."); s = defaultSlot(S.selSlot); }
      const n = S.akwfFrames.frames.length;
      s.sound.wave = "table"; s.wavetable = S.akwfGroup; s.table = el.dataset.f;
      s.sound.position = n > 1 ? +el.dataset.wi / (n - 1) : 0;
      selectApply(s);
    });
  });
}

// Slot updates go out one at a time, in order (a slider's debounced update can never land
// after a later selection). The body is taken when it is sent, and the fields pilabd
// resolves (name, source, and position when it picked the wave) are copied back into the
// object the editor is bound to, so later edits build on the server's state.
let pushChain = Promise.resolve();
function pushSlot(s, rerender = false) {
  clearTimeout(timers.slot);
  const job = pushChain.then(async () => {
    const sentTable = s.table;
    try {
      const r = await put(`/scene/slots/${s.id}`, s);
      S.scene = norm(r.scene); S.dirty = r.dirty;
      const f = slotById(s.id);
      if (f) {
        Object.assign(s, { name: f.name, name_locked: !!f.name_locked, table: f.table, wavetable: f.wavetable, sample: f.sample });
        s.sound.wave = f.sound.wave;
        if (f.table !== sentTable) s.sound.position = f.sound.position;
        if (f.sound.wave === "table" && f.wavetable) S.akwfGroup = f.wavetable;
      }
      if (rerender) renderSounds(); else if (s.sound.wave === "table") drawWave();
      return f;
    } catch (e) { fail(e); return null; }
  });
  pushChain = job.catch(() => {});
  return job;
}
// A selection (type, group, wave, bank): apply it, read the slot back, then preview.
async function selectApply(s, loadingMsg = "") {
  const want = s.sound.wave;
  if (loadingMsg) setLoading(true, loadingMsg);
  const f = await pushSlot(s, true);
  setLoading(false);
  if (!f) return;
  if (f.sound.wave !== want) return toast(`Not applied: slot is ${f.sound.wave}`, true);
  if (S.prevSel) audition({ slot: f.id });
  else if (loadingMsg) toast(`${f.name} ready`);
}

function cvCtx(cv) { const c = cv.getContext("2d"); c.clearRect(0, 0, cv.width, cv.height); return c; }
function css(v) { return getComputedStyle(document.documentElement).getPropertyValue(v).trim(); }

async function drawWave() {
  const cv = $("#wave-cv"); if (!cv) return;
  let pts = [];
  try { pts = (await get(`/scene/slots/${S.selSlot}/wave?points=450`)).points; } catch (e) { return; }
  const c = cvCtx(cv), w = cv.width, h = cv.height;
  c.strokeStyle = css("--line"); c.lineWidth = 1;
  c.beginPath(); c.moveTo(0, h / 2); c.lineTo(w, h / 2); c.stroke();
  const peak = Math.max(1, ...pts.map(Math.abs));
  c.strokeStyle = css("--text"); c.lineWidth = 2.5; c.lineJoin = "round"; c.beginPath();
  pts.concat(pts).forEach((v, i) => { const x = i / (pts.length * 2 - 1) * w, y = h / 2 - v / peak * h * 0.42; i ? c.lineTo(x, y) : c.moveTo(x, y); });
  c.stroke();
}

function drawEnv(id, e) {
  const cv = document.getElementById(id); if (!cv) return;
  const c = cvCtx(cv), w = cv.width, h = cv.height, pad = 8;
  const hold = 0.25 * (e.a + e.d + e.r) + 0.1, total = e.a + e.d + hold + e.r;
  const X = (t) => pad + t / total * (w - 2 * pad), Y = (v) => h - pad - v * (h - 2 * pad);
  c.strokeStyle = css("--text-2"); c.lineWidth = 2; c.beginPath();
  c.moveTo(X(0), Y(0)); c.lineTo(X(e.a), Y(1)); c.lineTo(X(e.a + e.d), Y(e.s)); c.lineTo(X(e.a + e.d + hold), Y(e.s)); c.lineTo(X(total), Y(0)); c.stroke();
}

async function drawPreviews() {
  for (const cv of $$("canvas[data-prev]")) {
    try {
      const pts = (await get(`/library/preview?path=${encodeURIComponent(cv.dataset.prev)}&points=64`)).points;
      const c = cvCtx(cv), w = cv.width, h = cv.height;
      c.strokeStyle = css("--text-2"); c.lineWidth = 2; c.beginPath();
      pts.forEach((v, i) => { const x = i / (pts.length - 1) * w, y = h / 2 - v * h * 0.42; i ? c.lineTo(x, y) : c.moveTo(x, y); });
      c.stroke();
    } catch (e) { break; }
    if (S.view !== "sounds") break;
  }
}

// ------------------------------------------------------------------ MIDI map
function inputChoices() {
  const fixed = [{ v: "*", l: "Any input" }, { v: "console", l: "Web keyboard" }, { v: "virtual", l: "Virtual port (apps)" }]
    .concat((S.devices || []).map((d) => ({ v: "device:" + d.id, l: d.label + " (device)" })));
  const hw = (S.inputs || []).filter((i) => i.slot >= 1 && i.slot <= 7).map((i) => ({ v: `*${i.label}*`, l: i.label }));
  return fixed.concat(hw);
}

function renderMidiMap() {
  if (!S.scene) return;
  const sc = S.scene, slots = sc.slots;
  const rows = inputChoices();
  for (const r of sc.routes) if (!rows.some((x) => x.v === r.input)) rows.push({ v: r.input, l: r.input });
  const portOf = (v) => v === "console" ? 8 : v === "virtual" ? 0 : v === "*" ? null : v.startsWith("device:") ? ((S.inputs || []).find((i) => i.slot != null && i.device_id === v.slice(7)) || {}).slot : ((S.inputs || []).find((i) => `*${i.label}*` === v) || {}).slot;
  const matrix = `<table class="matrix"><thead><tr><th></th>${slots.map((s) => `<th class="col">${s.id} · ${esc(s.name)}</th>`).join("")}</tr></thead><tbody>
    ${rows.map((r) => `<tr><td class="src">${portOf(r.v) != null ? `<span class="act" data-port="${portOf(r.v)}"></span>` : `<span class="act"></span>`}${esc(r.l)}</td>
      ${slots.map((s) => {
        const rs = sc.routes.filter((x) => x.input === r.v && x.slot === s.id && x.enabled);
        const full = rs.some((x) => x.channel === 0 && x.note_lo === 0 && x.note_hi === 127 && x.transpose === 0);
        return `<td class="cell ${full ? "on" : rs.length ? "part" : ""}" data-in="${esc(r.v)}" data-slot="${s.id}" title="${rs.length ? rs.length + " route(s)" : "click to route"}"><i></i></td>`;
      }).join("")}</tr>`).join("")}
  </tbody></table>`;
  const slotOpts = (sel) => slots.map((s) => `<option value="${s.id}" ${s.id === sel ? "selected" : ""}>${s.id} · ${esc(s.name)}</option>`).join("");
  const inOpts = (sel) => rows.map((r) => `<option value="${esc(r.v)}" ${r.v === sel ? "selected" : ""}>${esc(r.l)}</option>`).join("");
  const chOpts = (sel) => `<option value="0" ${sel === 0 ? "selected" : ""}>omni</option>` + Array.from({ length: 16 }, (_, i) => `<option value="${i + 1}" ${sel === i + 1 ? "selected" : ""}>ch ${i + 1}</option>`).join("");
  const routes = sc.routes.map((r, i) => `<tr data-i="${i}">
    <td><label class="switch"><input type="checkbox" data-k="enabled" ${r.enabled ? "checked" : ""}><span></span></label></td>
    <td><input class="input w-md" data-k="label" value="${esc(r.label)}" placeholder="label"></td>
    <td><select data-k="input">${inOpts(r.input)}</select></td>
    <td><select data-k="channel">${chOpts(r.channel)}</select></td>
    <td><input class="input mono w-sm" data-k="note_lo" value="${noteName(r.note_lo)}" title="lowest note (name or number)"></td>
    <td><input class="input mono w-sm" data-k="note_hi" value="${noteName(r.note_hi)}" title="highest note"></td>
    <td><input class="input mono w-sm" type="number" min="-48" max="48" data-k="transpose" value="${r.transpose}"></td>
    <td><input class="input mono w-sm" type="number" min="0" max="2" step="0.05" data-k="velocity_scale" value="${r.velocity_scale}"></td>
    <td class="nowrap"><select data-k="slot">${slotOpts(r.slot)}</select> ${playBtn(`data-play="slot:${r.slot}"`, "Play this sound")}</td>
    <td><div class="range-bar"><i style="left:${r.note_lo / 127 * 100}%;width:${Math.max(1, (r.note_hi - r.note_lo) / 127 * 100)}%"></i></div></td>
    <td><button class="btn ghost small" data-del="${i}">✕</button></td></tr>`).join("");
  // keyboard lanes: one lane per slot showing which notes reach it (splits and layers)
  const lanes = slots.map((s) => {
    const rs = sc.routes.filter((r) => r.enabled && r.slot === s.id);
    return `<div class="lane"><span class="small">${s.id} · ${esc(s.name)}</span><div class="bar">${rs.map((r, k) => `<i class="l${k % 3}" style="left:${r.note_lo / 127 * 100}%;width:${Math.max(0.8, (r.note_hi - r.note_lo + 1) / 128 * 100)}%" title="${esc(r.input)} ${r.channel ? "ch" + r.channel : "omni"} ${noteName(r.note_lo)}–${noteName(r.note_hi)}${r.transpose ? " " + (r.transpose > 0 ? "+" : "") + r.transpose : ""}"></i>`).join("")}</div></div>`;
  }).join("");
  const axis = [0, 24, 36, 48, 60, 72, 84, 96, 108, 127].map((n) => `<span style="left:${n / 127 * 100}%">${noteName(n)}</span>`).join("");
  $("#main").innerHTML = head("MIDI Map", "Inputs × channels × note ranges → sounds. Overlapping routes layer; separate ranges split. Changes apply live.", `<button class="btn ghost" id="add-route">Add route</button><button class="btn primary" id="save-now">Save scene</button>`) + `
  <div class="grid g3">
    <div class="card span2"><div class="card-head"><h3>Routing matrix</h3><span class="spacer"></span><span class="muted small">● full-range route · ◍ split or channel-specific · click to toggle a full-range route</span></div>${matrix}</div>
    <div class="card"><div class="card-head"><h3>MIDI monitor</h3><span class="spacer"></span><button class="btn ghost small" id="mon-clear">Clear</button></div><div class="monitor" id="monitor"></div></div>
    <div class="card span4"><div class="card-head"><h3>Routes</h3><span class="spacer"></span><span class="muted small">Inputs: a known device (<code>device:trillium</code>, from Settings), <code>console</code>, <code>virtual</code>, or a glob on the source name</span></div>
      <div style="overflow-x:auto"><table class="routes"><thead><tr><th>On</th><th>Label</th><th>Input</th><th>Channel</th><th>Low</th><th>High</th><th>Transpose</th><th>Vel ×</th><th>Sound</th><th>Range</th><th></th></tr></thead><tbody>${routes || `<tr><td colspan="11" class="muted">No routes. Add one, or click a matrix cell.</td></tr>`}</tbody></table></div></div>
    <div class="card span4"><div class="card-head"><h3>Keyboard lanes</h3><span class="spacer"></span><span class="muted small">Which notes reach each sound</span></div>
      <div class="keys-axis"><span></span><div class="ax">${axis}</div></div><div class="lanes">${lanes}</div></div>
  </div>`;
  updateMonitor();
  $("#save-now").addEventListener("click", () => saveScene(S.scene.name));
  $("#mon-clear").addEventListener("click", () => { S.midiLog = []; updateMonitor(); });
  bindPlay($("#main")); bindOutput(renderMidiMap);
  $("#add-route").addEventListener("click", () => { const r = { enabled: true, label: "", input: "*", channel: 0, note_lo: 0, note_hi: 127, transpose: 0, velocity_scale: 1, slot: (slots[0] || {}).id || 1 }; pushRoutes(sc.routes.concat([r])); });
  $$("td.cell").forEach((td) => td.addEventListener("click", () => {
    const inp = td.dataset.in, slot = +td.dataset.slot;
    const idx = sc.routes.findIndex((x) => x.input === inp && x.slot === slot && x.channel === 0 && x.note_lo === 0 && x.note_hi === 127 && x.transpose === 0);
    const next = sc.routes.slice();
    if (idx >= 0) next.splice(idx, 1); else next.push({ enabled: true, label: "", input: inp, channel: 0, note_lo: 0, note_hi: 127, transpose: 0, velocity_scale: 1, slot });
    pushRoutes(next);
  }));
  $$("table.routes [data-del]").forEach((b) => b.addEventListener("click", () => { const n = sc.routes.slice(); n.splice(+b.dataset.del, 1); pushRoutes(n); }));
  $$("table.routes [data-k]").forEach((el) => el.addEventListener("change", () => {
    const i = +el.closest("tr").dataset.i, k = el.dataset.k, n = sc.routes.map((r) => ({ ...r }));
    let v = el.type === "checkbox" ? el.checked : el.value;
    if (k === "note_lo" || k === "note_hi") { v = parseNote(v); if (v == null) return toast("Note: a name like C4 or a number 0–127", true); }
    else if (["channel", "transpose", "slot"].includes(k)) v = parseInt(v, 10);
    else if (k === "velocity_scale") v = parseFloat(v);
    n[i][k] = v; pushRoutes(n);
  }));
}

function parseNote(s) {
  s = String(s).trim();
  if (/^\d+$/.test(s)) { const n = +s; return n >= 0 && n <= 127 ? n : null; }
  const m = s.match(/^([A-Ga-g])(#|b)?(-?\d)$/);
  if (!m) return null;
  let n = { c: 0, d: 2, e: 4, f: 5, g: 7, a: 9, b: 11 }[m[1].toLowerCase()] + (m[2] === "#" ? 1 : m[2] === "b" ? -1 : 0);
  n += (parseInt(m[3], 10) + 1) * 12;
  return n >= 0 && n <= 127 ? n : null;
}

async function pushRoutes(routes) {
  try { const r = await put("/scene/routes", routes); S.scene = norm(r.scene); S.dirty = r.dirty; S.inputs = r.midi_inputs; if (r.devices) S.devices = r.devices; renderMidiMap(); } catch (e) { fail(e); }
}

function updateMonitor() {
  const el = $("#monitor"); if (!el) return;
  el.innerHTML = S.midiLog.slice(0, 120).map((m) => `<div><span class="t">${m.t.toFixed(2)}</span><span>${esc(m.source || "port " + m.port)}</span><span>ch${m.ch}</span><span>${esc(m.text)}</span></div>`).join("") || `<div class="muted" style="display:block">Play a controller or the keyboard below.</div>`;
}

// ------------------------------------------------------------------ scenes
async function renderScenes() {
  try { const r = await get("/scenes"); S.scenes = r.scenes; } catch (e) { fail(e); }
  const rows = S.scenes.map((s) => `<tr>
    <td>${s.name === S.scene.name ? `<span class="badge ok">active</span>` : ""}</td>
    <td style="font-family:var(--font-display);font-size:15px">${esc(s.name)}</td>
    <td class="muted">${s.error ? `<span class="badge caution">invalid</span> ${esc(s.error)}` : esc(s.description)}</td>
    <td class="num">${s.slots}</td><td class="num">${s.routes}</td>
    <td class="mono small muted">${new Date(s.modified * 1000).toLocaleString()}</td>
    <td style="text-align:right;white-space:nowrap">
      <button class="btn ghost small" data-load="${esc(s.name)}" ${s.error ? "disabled" : ""}>Load</button>
      <button class="btn ghost small" data-dup="${esc(s.name)}">Duplicate</button>
      <a class="btn ghost small" href="${API}/scenes/${encodeURIComponent(s.name)}" data-dl="${esc(s.name)}">TOML</a>
      <button class="btn ghost small" data-rm="${esc(s.name)}" ${s.name === S.scene.name ? "disabled" : ""}>Delete</button></td></tr>`).join("");
  $("#main").innerHTML = head("Scenes", "A scene is the whole setup: sound slots, MIDI routing, master level and tuning, saved as versioned TOML in <code>~/.config/pilab/scenes</code>.") + `
  <div class="grid g3">
    <div class="card span2"><div class="card-head"><h3>Saved scenes</h3></div>
      <table><thead><tr><th></th><th>Name</th><th>Description</th><th class="num">Sounds</th><th class="num">Routes</th><th>Modified</th><th></th></tr></thead><tbody>${rows}</tbody></table></div>
    <div class="card"><div class="card-head"><h3>Active scene</h3>${S.dirty ? `<span class="badge caution">unsaved changes</span>` : `<span class="badge ok">saved</span>`}</div>
      <div class="field"><label>Name</label><input class="input" id="sc-name" value="${esc(S.scene.name)}"></div>
      <div class="field" style="margin-top:8px"><label>Description</label><input class="input" id="sc-desc" value="${esc(S.scene.description)}"></div>
      <div class="row" style="margin-top:8px">
        <div class="field"><label>A4 (Hz)</label><input class="input mono" id="sc-a4" type="number" min="380" max="480" step="0.1" value="${S.scene.a4_hz}" style="width:100px"></div>
        <div class="field"><label>Bend range</label><input class="input mono" id="sc-bend" type="number" min="0" max="24" step="1" value="${S.scene.bend_range}" style="width:90px"></div>
      </div>
      <div class="row" style="margin-top:14px"><button class="btn primary" id="sc-save">Save scene</button><span class="muted small">Saving under a new name keeps the old scene.</span></div>
      <div class="risk" style="margin-top:14px">Loading a scene never raises the level above what is playing now, and the service always starts at −18 dBFS or lower.</div>
    </div>
  </div>`;
  $("#sc-save").addEventListener("click", async () => {
    try { await api("PATCH", "/scene", { description: $("#sc-desc").value, a4_hz: +$("#sc-a4").value, bend_range: +$("#sc-bend").value }); await saveScene($("#sc-name").value.trim()); } catch (e) { fail(e); }
  });
  $$("[data-load]").forEach((b) => b.addEventListener("click", async () => {
    if (S.dirty && !confirm("The active scene has unsaved changes. Load anyway?")) return;
    try { const r = await post(`/scenes/${encodeURIComponent(b.dataset.load)}/load`); S.scene = norm(r.scene); S.dirty = r.dirty; toast(`Loaded ${b.dataset.load}`); renderScenes(); } catch (e) { fail(e); }
  }));
  $$("[data-dup]").forEach((b) => b.addEventListener("click", async () => {
    const to = prompt("Name for the copy", b.dataset.dup + " copy"); if (!to) return;
    try { await post(`/scenes/${encodeURIComponent(b.dataset.dup)}/duplicate`, { to }); renderScenes(); } catch (e) { fail(e); }
  }));
  $$("[data-rm]").forEach((b) => b.addEventListener("click", async () => {
    if (!confirm(`Delete scene "${b.dataset.rm}"? A .bak copy of the last save stays on the device.`)) return;
    try { await api("DELETE", `/scenes/${encodeURIComponent(b.dataset.rm)}`); renderScenes(); } catch (e) { fail(e); }
  }));
  $$("[data-dl]").forEach((a) => a.addEventListener("click", async (e) => {
    e.preventDefault();
    try { const t = await get(`/scenes/${encodeURIComponent(a.dataset.dl)}`); const u = URL.createObjectURL(new Blob([t], { type: "text/plain" })); const x = document.createElement("a"); x.href = u; x.download = a.dataset.dl + ".toml"; x.click(); URL.revokeObjectURL(u); } catch (er) { fail(er); }
  }));
}

async function saveScene(name) {
  if (!name) return toast("Scene name required", true);
  try { await post(`/scenes/${encodeURIComponent(name)}/save`); await refreshScene(); toast(`Saved scene ${name}`); if (S.view === "scenes") renderScenes(); } catch (e) { fail(e); }
}

// ------------------------------------------------------------------ signals
let selPort = null;
async function renderSignals() {
  try { [S.graph, S.adev] = await Promise.all([get("/graph"), get("/audio/devices").catch(() => ({ sinks: [], sources: [] }))]); } catch (e) { fail(e); return; }
  const ports = S.graph.ports;
  const group = (dir) => {
    const m = new Map();
    for (const p of ports.filter((x) => x.dir === dir && (x.kind === "audio" || x.kind === "midi"))) { if (!m.has(p.client)) m.set(p.client, []); m.get(p.client).push(p); }
    return Array.from(m.entries()).sort((a, b) => (a[0] === "pilab-synth" ? -1 : b[0] === "pilab-synth" ? 1 : a[0].localeCompare(b[0])));
  };
  const col = (dir) => group(dir).map(([c, ps]) => `<div class="client"><div class="cn">${esc(c)}${c === "pilab-synth" ? ` <span class="badge">this synth</span>` : ""}</div>
    ${ps.map((p) => `<div class="port ${p.kind} ${selPort === p.name ? "sel" : ""}" data-port="${esc(p.name)}" data-dir="${dir}" data-kind="${p.kind}">${dir === "in" ? `<span>${esc(p.short)}</span><span class="pd"></span>` : `<span class="pd"></span><span>${esc(p.short)}</span>`}</div>`).join("")}</div>`).join("");
  const links = ports.filter((p) => p.dir === "out").flatMap((p) => p.connections.map((c) => [p.name, c]));
  $("#main").innerHTML = head("Signals", `Audio and MIDI graph (PipeWire via the JACK API, ${esc(S.graph.backend)}). Click an output, then an input, to link or unlink them.`, `<button class="btn ghost" id="sig-refresh">Refresh</button>`) + `
  <div class="grid g4">
    <div class="card span3"><div class="patch" id="patch"><div class="col out">${col("out")}</div><div></div><div class="col in">${col("in")}</div><svg id="patch-svg"></svg></div></div>
    <div class="card"><div class="card-head"><h3>Links</h3><span class="spacer"></span><span class="mono muted">${links.length}</span></div>
      <div class="tbl-scroll" style="max-height:600px"><table><tbody>${links.map(([a, b]) => `<tr><td class="small">${esc(a)}<br><span class="muted">→ ${esc(b)}</span></td><td><button class="btn ghost small" data-unlink="${esc(a)}|${esc(b)}">✕</button></td></tr>`).join("") || `<tr><td class="muted">No links</td></tr>`}</tbody></table></div>
      <div class="muted small" style="margin-top:10px">○ MIDI · ● audio. Hardware MIDI sources are connected to <span class="mono">pilab-synth:in_N</span> automatically; apps can use <span class="mono">pilab-synth:midi_in</span>.</div></div>
    <div class="card span2"><div class="card-head"><h3>Audio inputs (sources)</h3><span class="spacer"></span><span class="muted small">capture devices: USB mics and interfaces</span></div>
      <table><tbody>${(S.adev.sources || []).map((d) => `<tr><td>${esc(d.description || d.name)}<div class="mono small muted">${esc(d.name)}</div></td><td><span class="badge">${esc(d.kind)}</span></td><td class="small muted">${esc(String(d.state || "").toLowerCase())}</td></tr>`).join("") || `<tr><td class="muted">No capture devices. Plug in a USB mic or audio interface and it shows here. AirPlay speakers are never inputs.</td></tr>`}</tbody></table></div>
    <div class="card span2"><div class="card-head"><h3>Audio outputs (sinks)</h3><span class="spacer"></span><span class="muted small">where sound can go</span></div>
      <table><tbody>${(S.adev.sinks || []).map((d) => `<tr><td>${esc(d.description || d.name)}<div class="mono small muted">${esc(d.name)}</div></td><td><span class="badge ${d.airplay ? "caution" : ""}">${esc(d.kind)}</span></td><td class="small muted">${esc(String(d.state || "").toLowerCase())}</td></tr>`).join("") || `<tr><td class="muted">No outputs reported</td></tr>`}</tbody></table>
      <div class="muted small" style="margin-top:8px">AirPlay speakers (HomePods, Macs) appear only while the AirPlay sender is on. They are outputs only, never inputs.</div></div>
  </div>`;
  drawLinks(links);
  $("#sig-refresh").addEventListener("click", renderSignals);
  $$("#patch .port").forEach((el) => el.addEventListener("click", async () => {
    if (el.dataset.dir === "out") { selPort = selPort === el.dataset.port ? null : el.dataset.port; renderSignals(); return; }
    if (!selPort) return toast("Pick an output port first");
    const linked = links.some(([a, b]) => a === selPort && b === el.dataset.port);
    try { await post(linked ? "/graph/disconnect" : "/graph/connect", { from: selPort, to: el.dataset.port }); toast(linked ? "Unlinked" : "Linked"); } catch (e) { fail(e); }
    renderSignals();
  }));
  $$("[data-unlink]").forEach((b) => b.addEventListener("click", async () => { const [from, to] = b.dataset.unlink.split("|"); try { await post("/graph/disconnect", { from, to }); renderSignals(); } catch (e) { fail(e); } }));
}

function drawLinks(links) {
  const svg = $("#patch-svg"), box = $("#patch"); if (!svg) return;
  const R = box.getBoundingClientRect();
  const find = (name, dir) => $$(`#patch .port[data-dir="${dir}"]`).find((e) => e.dataset.port === name);
  svg.innerHTML = links.map(([a, b]) => {
    const ea = find(a, "out"), eb = find(b, "in"); if (!ea || !eb) return "";
    const ra = ea.getBoundingClientRect(), rb = eb.getBoundingClientRect();
    const x1 = ra.right - R.left, y1 = ra.top + ra.height / 2 - R.top, x2 = rb.left - R.left, y2 = rb.top + rb.height / 2 - R.top;
    const hot = a === selPort;
    return `<path d="M${x1},${y1} C${x1 + 70},${y1} ${x2 - 70},${y2} ${x2},${y2}" fill="none" stroke="${hot ? css("--accent") : css("--text-3")}" stroke-width="${hot ? 2 : 1.25}" opacity="${hot ? 1 : 0.8}"/>`;
  }).join("");
}

// ------------------------------------------------------------------ settings
async function renderSettings() {
  let r, recs;
  try { r = await get("/settings"); recs = await get("/recordings"); } catch (e) { fail(e); return; }
  const st = r.settings; S.settings = st;
  const sys = S.sys || {};
  const recRows = recs.recordings.slice(0, 12).map((c) => `<tr><td class="mono small">${esc(c.id)}</td><td class="muted small">${esc(c.created.replace("T", " ").slice(0, 19))}</td><td class="num">${c.duration_s.toFixed(1)} s</td><td>${c.filed ? `<span class="badge ok">filed</span>` : `<span class="badge caution">only on this device</span>`}</td></tr>`).join("") || `<tr><td class="muted">No recordings yet.</td></tr>`;
  const locs = ["~/.local/share/pilab/recordings", "/srv/library/pilab-library/recordings"];
  if (!locs.includes(st.recording.location)) locs.push(st.recording.location);
  const urls = [`http://${(sys.hostname || "pilab")}.local:${sys.port || 8410}/`].concat((sys.addresses || []).filter((a) => !a.includes(":")).map((a) => `http://${a}:${sys.port || 8410}/`));
  $("#main").innerHTML = head("Settings", "Recording, archival, audio, MIDI and network. Each switch shows what it changes.", `<button class="btn primary" id="set-save">Save settings</button>`) + `
  <div class="grid g2">
    <div class="card">
      <div class="card-head"><h3>Local recording</h3><span class="spacer"></span><label class="switch"><input type="checkbox" id="rec-en" ${st.recording.enabled ? "checked" : ""}><span></span></label></div>
      <div class="risk">Records the synth output (post limiter) as 32-bit float WAV with a record card. Local may be enough; recordings are masters until filed to the NAS Archive.</div>
      <div class="row" style="margin-top:10px">
        <div class="field" style="flex:1"><label>Location</label><select id="rec-loc">${locs.map((l) => `<option ${l === st.recording.location ? "selected" : ""}>${esc(l)}</option>`).join("")}</select></div>
        <div class="field"><label>Stop below (MB free)</label><input class="input mono" id="rec-min" type="number" min="256" value="${st.recording.min_free_mb}" style="width:110px"></div>
        <div class="field"><label>Warn unfiled after (h)</label><input class="input mono" id="rec-warn" type="number" min="1" value="${st.recording.unfiled_warn_hours}" style="width:110px"></div>
      </div>
      <div class="lbl" style="margin:14px 0 4px">Recent recordings · <span class="mono" style="text-transform:none;letter-spacing:0">${esc(recs.location)}</span></div>
      <table><tbody>${recRows}</tbody></table>
    </div>
    <div class="card">
      <div class="card-head"><h3>Archive to NAS</h3><span class="spacer"></span><span class="badge ${r.archive_state === "ready" ? "ok" : r.archive_state === "off" ? "off" : "caution"}">${esc(r.archive_state)}</span><label class="switch"><input type="checkbox" id="arc-en" ${st.archive.enabled ? "checked" : ""}><span></span></label></div>
      <div class="risk ${st.archive.enabled ? "caution" : ""}">${st.archive.enabled ? "<b>!</b> " : ""}${esc(r.archive_text)}</div>
      <div class="row" style="margin-top:10px">
        <div class="field"><label>Mode</label><select id="arc-mode"><option value="manual" ${st.archive.mode === "manual" ? "selected" : ""}>Manual (file from the console)</option><option value="after-each" ${st.archive.mode === "after-each" ? "selected" : ""}>After each recording</option></select></div>
        <div class="field" style="flex:1"><label>Target share</label><input class="input mono" id="arc-target" value="${esc(st.archive.target)}"></div>
      </div>
      <div class="kv" style="margin-top:10px">
        <span class="k">Account</span><span class="v">${esc(st.archive.account)} (write-only to that share)</span>
        <span class="k">Credentials</span><span class="v">${esc(st.archive.credentials_file)} (root-only, never shown)</span>
        <span class="k">Files land in</span><span class="v">PilabRecordings → Archive/topics/pilab/recordings/{record-id}/</span>
      </div>
      <div class="muted small" style="margin-top:8px">Nothing is mounted or copied until the NAS account exists and a test login passes. Archive copies never delete the local master automatically.</div>
    </div>
    <div class="card">
      <div class="card-head"><h3>Audio</h3></div>
      <div class="field"><label>Synth outputs connect to</label><input class="input mono" id="aud-out" value="${esc(st.audio.output_ports.join(", "))}"></div>
      <div class="field" style="margin-top:8px"><label>DAC sink (watchdog)</label><input class="input mono" id="aud-sink" value="${esc(st.audio.dac_sink)}"></div>
      <div class="row" style="margin-top:10px"><label class="switch"><input type="checkbox" id="aud-start" ${st.audio.start_engine ? "checked" : ""}><span></span></label><span>Start the synth when the service starts (fade-in, −18 dBFS max)</span></div>
      <div class="lbl" style="margin:14px 0 6px">Dropout watchdog</div>
      <div class="row"><label class="switch"><input type="checkbox" id="wd-en" ${st.watchdog.enabled ? "checked" : ""}><span></span></label><span>Suspend and resume the DAC sink after</span>
        <input class="input mono" id="wd-rate" type="number" min="1" step="1" value="${st.watchdog.xruns_per_s}" style="width:70px"><span>dropouts/s for</span>
        <input class="input mono" id="wd-hold" type="number" min="2" step="1" value="${st.watchdog.hold_s}" style="width:70px"><span>s</span></div>
      <div class="muted small" style="margin-top:6px">Causes about one second of silence on the DAC when it fires. Cooldown ${st.watchdog.cooldown_s} s.</div>
    </div>
    <div class="card">
      <div class="card-head"><h3>MIDI</h3></div>
      <div class="row"><label class="switch"><input type="checkbox" id="midi-auto" ${st.midi.autoconnect ? "checked" : ""}><span></span></label><span>Connect new MIDI sources automatically (Trillium and other USB MIDI devices, ALSA apps)</span></div>
      <div class="field" style="margin-top:8px"><label>Never connect (globs, comma separated)</label><input class="input mono" id="midi-ignore" value="${esc(st.midi.ignore.join(", "))}" placeholder="*Midi Through*"></div>
      <div class="lbl" style="margin:14px 0 6px">Known devices (matched in order)</div>
      ${st.midi.devices.map((d) => `<div class="kv" style="margin-bottom:8px">
        <span class="k">${esc(d.label)}</span><span class="v">device:${esc(d.id)}</span>
        <span class="k">USB ids</span><span class="v">${esc(d.usb_ids.join(", ") || "—")}</span>
        <span class="k">Name starts with</span><span class="v">${esc(d.name_prefixes.join(", ") || "—")}</span>
        <span class="k">BLE alias</span><span class="v">${esc(d.ble_aliases.join(", ") || "—")}</span>
        <span class="k">Routed cables</span><span class="v">${d.route_cables.length ? d.route_cables.join(", ") : "all"}</span>
      </div>${d.note ? `<div class="muted small" style="margin-bottom:8px">${esc(d.note)}</div>` : ""}`).join("")}
      <div class="muted small">Edit <code>[[midi.devices]]</code> in <code>~/.config/pilab/settings.toml</code> on the Pi, then restart pilabd.</div>
      <div class="lbl" style="margin:14px 0 6px">Network and pairing</div>
      <div class="kv">
        <span class="k">Console</span><span class="v">${urls.map(esc).join("<br>")}</span>
        <span class="k">mDNS</span><span class="v">_pilab._tcp · _http._tcp</span>
        <span class="k">Device id</span><span class="v">${esc(sys.device_id || "")}</span>
      </div>
      <div class="risk caution" style="margin-top:10px"><b>!</b> The console is plain HTTP on the LAN. Anyone on this network with the pairing token can change the sound. On venue networks, prefer a direct cable.</div>
      <div class="row" style="margin-top:10px"><button class="btn ghost small" id="forget">Forget token in this browser</button><span class="muted small">New token on the Pi: <code>pilabctl pair --rotate</code></span></div>
    </div>
  </div>`;
  $("#arc-en").addEventListener("change", (e) => toast(e.target.checked ? "Archival on: copies recordings to the NAS share once it is configured. It is not configured yet, so nothing will be copied." : "Archival off: recordings stay on this device."));
  $("#rec-en").addEventListener("change", (e) => toast(e.target.checked ? "Local recording allowed. Uses space on the selected drive." : "Local recording off."));
  $("#forget").addEventListener("click", () => { localStorage.removeItem("pilab.token"); S.token = ""; showPair("Token forgotten."); });
  $("#set-save").addEventListener("click", async () => {
    const n = JSON.parse(JSON.stringify(st));
    n.recording.enabled = $("#rec-en").checked; n.recording.location = $("#rec-loc").value; n.recording.min_free_mb = +$("#rec-min").value; n.recording.unfiled_warn_hours = +$("#rec-warn").value;
    n.archive.enabled = $("#arc-en").checked; n.archive.mode = $("#arc-mode").value; n.archive.target = $("#arc-target").value.trim();
    n.audio.output_ports = $("#aud-out").value.split(",").map((s) => s.trim()).filter(Boolean); n.audio.dac_sink = $("#aud-sink").value.trim(); n.audio.start_engine = $("#aud-start").checked;
    n.watchdog.enabled = $("#wd-en").checked; n.watchdog.xruns_per_s = +$("#wd-rate").value; n.watchdog.hold_s = +$("#wd-hold").value;
    n.midi.autoconnect = $("#midi-auto").checked; n.midi.ignore = $("#midi-ignore").value.split(",").map((s) => s.trim()).filter(Boolean);
    try { await put("/settings", n); toast("Settings saved"); renderSettings(); } catch (e) { fail(e); }
  });
}

// ------------------------------------------------------------------ keyboard
const KEYMAP = "awsedftgyhujkolp;'";
const held = new Set();
function buildKeyboard() {
  const k = $("#keys"); const octs = 3, base = 0;
  const whites = [0, 2, 4, 5, 7, 9, 11];
  let html = "";
  const nW = octs * 7 + 1;
  for (let o = 0; o < octs; o++) for (const w of whites) html += `<div class="w" data-off="${o * 12 + w}">${w === 0 ? `<span data-lbl="${o}"></span>` : ""}</div>`;
  html += `<div class="w" data-off="${octs * 12}"><span data-lbl="${octs}"></span></div>`;
  k.innerHTML = html;
  const bl = [1, 3, 6, 8, 10], pos = { 1: 1, 3: 2, 6: 4, 8: 5, 10: 6 };
  for (let o = 0; o < octs; o++) for (const b of bl) {
    const d = document.createElement("div"); d.className = "b"; d.dataset.off = o * 12 + b;
    const left = (o * 7 + pos[b]) / nW * 100; d.style.left = `calc(${left}% - 1.1%)`; d.style.width = "2.2%"; k.appendChild(d);
  }
  updateOct();
  const down = (el) => { if (!el || !el.dataset.off) return; const n = S.kbdOct * 12 + 12 + +el.dataset.off - 12; noteOn(n + 0); };
  let ptr = null;
  k.addEventListener("pointerdown", (e) => { const el = e.target.closest("[data-off]"); if (!el) return; k.setPointerCapture(e.pointerId); ptr = el; noteOn(keyNote(el)); });
  k.addEventListener("pointermove", (e) => { if (!ptr) return; const el = document.elementFromPoint(e.clientX, e.clientY); const t = el && el.closest("[data-off]"); if (t && t !== ptr) { noteOff(keyNote(ptr)); ptr = t; noteOn(keyNote(t)); } });
  const up = () => { if (ptr) { noteOff(keyNote(ptr)); ptr = null; } };
  k.addEventListener("pointerup", up); k.addEventListener("pointercancel", up);
  void down;
}
const keyNote = (el) => (S.kbdOct + 1) * 12 + +el.dataset.off;
function updateOct() { $("#oct-v").textContent = "C" + S.kbdOct; $$("[data-lbl]").forEach((s) => s.textContent = "C" + (S.kbdOct + +s.dataset.lbl)); }
function markKey(n, on) { const off = n - (S.kbdOct + 1) * 12; const el = $(`#keys [data-off="${off}"]`); if (el) el.classList.toggle("down", on); }
function sendMidi(msg) {
  msg.t = "midi"; msg.target = S.kbdTarget;
  if (S.ws && S.ws.readyState === 1) S.ws.send(JSON.stringify(msg)); else post("/midi", msg).catch(fail);
}
function noteOn(n) { if (n < 0 || n > 127 || held.has(n)) return; held.add(n); markKey(n, true); sendMidi({ note: n, vel: +$("#kbd-vel").value, on: true }); }
function noteOff(n) { if (!held.has(n)) return; held.delete(n); markKey(n, false); sendMidi({ note: n, on: false }); }

$("#btn-kbd").addEventListener("click", () => { $("#kbd").classList.toggle("hidden"); });
$("#oct-dn").addEventListener("click", () => { S.kbdOct = Math.max(0, S.kbdOct - 1); updateOct(); });
$("#oct-up").addEventListener("click", () => { S.kbdOct = Math.min(7, S.kbdOct + 1); updateOct(); });
$("#kbd-vel").addEventListener("input", (e) => $("#kbd-vel-v").textContent = e.target.value);
$("#kbd-bend").addEventListener("input", (e) => sendMidi({ bend: +e.target.value }));
const springBack = (e) => { e.target.value = 0; sendMidi({ bend: 0 }); };
$("#kbd-bend").addEventListener("pointerup", springBack); $("#kbd-bend").addEventListener("change", springBack);
$("#kbd-mod").addEventListener("input", (e) => sendMidi({ cc: 1, value: +e.target.value }));
$$("#kbd-target button").forEach((b) => b.addEventListener("click", () => {
  S.kbdTarget = b.dataset.t; $$("#kbd-target button").forEach((x) => x.classList.toggle("on", x === b));
  if (S.kbdTarget === "sound") post("/audition", { slot: S.selSlot }).catch(fail);
}));
window.addEventListener("keydown", (e) => {
  if (e.repeat || e.metaKey || e.ctrlKey || e.altKey) return;
  const t = e.target; if (t && (t.tagName === "INPUT" || t.tagName === "SELECT" || t.tagName === "TEXTAREA")) return;
  if ($("#kbd").classList.contains("hidden")) return;
  const k = e.key.toLowerCase();
  if (k === "z") { S.kbdOct = Math.max(0, S.kbdOct - 1); updateOct(); return; }
  if (k === "x") { S.kbdOct = Math.min(7, S.kbdOct + 1); updateOct(); return; }
  const i = KEYMAP.indexOf(k); if (i < 0) return;
  e.preventDefault(); noteOn((S.kbdOct + 1) * 12 + i);
});
window.addEventListener("keyup", (e) => { const i = KEYMAP.indexOf(e.key.toLowerCase()); if (i >= 0) noteOff((S.kbdOct + 1) * 12 + i); });
window.addEventListener("blur", () => { for (const n of Array.from(held)) noteOff(n); });
window.addEventListener("resize", () => { if (S.view === "signals" && S.graph) renderSignals(); });

boot();
