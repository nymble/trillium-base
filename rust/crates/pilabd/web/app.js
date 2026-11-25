// Copyright (C) 2026 M-of-N. SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
// Trillium Console web UI. Vanilla JS, no build step; served from the pilabd binary.
"use strict";

const API = "/api/v1";
const PRODUCT = document.body.dataset.product || "Trillium Console";
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
  t.textContent = msg; t.className = "toast" + (err ? " err" : ""); t.setAttribute("role", err ? "alert" : "status");
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
    <button data-o="dac" class="${S.out === "dac" ? "on" : ""}" title="DAC: headphone / line out of this Trillium Base, at the current master">DAC</button>
    <button data-o="browser" class="${S.out === "browser" ? "on" : ""}" title="Render on the device, play in this browser">This browser</button></div>
    <span class="row tight muted small" title="Play a short phrase whenever you select a sound or table"><span>Preview on select</span><label class="switch"><input type="checkbox" id="prev-sel" aria-label="Preview on select" ${S.prevSel ? "checked" : ""}><span></span></label></span></div>`;
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
// One-time pairing code (from `trillium-base pair --qr`): exchange it for the token.
async function redeemCode(code) {
  const r = await fetch(API + "/pair/redeem", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ code }) });
  if (!r.ok) return false;
  const j = await r.json();
  if (!j.token) return false;
  S.token = j.token;
  localStorage.setItem("pilab.token", S.token);
  return true;
}
const looksLikeCode = (v) => v.replace(/[^A-Za-z0-9]/g, "").length === 8;
$("#pair-form").addEventListener("submit", async (ev) => {
  ev.preventDefault();
  const v = $("#pair-token").value.trim();
  if (looksLikeCode(v)) {
    try {
      if (await redeemCode(v)) { $("#pair").classList.add("hidden"); start(); return; }
    } catch (e) { /* fall through */ }
    $("#pair-msg").textContent = "Code not accepted (expired, used or mistyped). Run trillium-base pair --qr on the device for a new one.";
    return;
  }
  S.token = v;
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
    $("#foot-ver").textContent = `Console v${S.info.version} · ${S.info.device_id}`;
    $("#foot-ver").title = `${S.info.product} ${S.info.version}, commit ${S.info.commit}`;
  }
  const pm = location.hash.match(/pair=([A-Za-z0-9-]+)/);
  if (pm) {
    history.replaceState(null, "", location.pathname + location.search);
    let ok = false;
    try { ok = await redeemCode(pm[1]); } catch (e) { ok = false; }
    if (!ok && !S.token) return showPair("That pairing code has expired or was already used. Run trillium-base pair --qr on the device for a new one.");
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
  if (d.input && $("#in-l")) {
    $("#in-l").style.width = pct(d.input.peak_db[0]) + "%";
    $("#in-r").style.width = pct(d.input.peak_db[1]) + "%";
  }
  const m = $("#master");
  if (document.activeElement !== m) { m.value = d.master_db; $("#master-v").textContent = fmtDb(d.master_db); }
  const rec = $("#btn-rec");
  rec.classList.toggle("on", d.rec.recording);
  rec.setAttribute("aria-pressed", d.rec.recording ? "true" : "false");
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
    const f = document.activeElement;
    if (!(f && $("#main").contains(f) && f.matches(":focus-visible"))) renderOverview();
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

const VIEW_TITLE = { overview: "Overview", sounds: "Sounds", midimap: "MIDI Map", scenes: "Scenes", signals: "Signals", devices: "Devices", settings: "Settings" };
function render() {
  $$("#nav a").forEach((a) => { const on = a.dataset.view === S.view; a.classList.toggle("on", on); if (on) a.setAttribute("aria-current", "page"); else a.removeAttribute("aria-current"); });
  document.title = `${VIEW_TITLE[S.view] || "Overview"} · ${PRODUCT} · ${(S.info && S.info.device) || "Trillium Base"}`;
  const views = { overview: renderOverview, sounds: renderSounds, midimap: renderMidiMap, scenes: renderScenes, signals: renderSignals, devices: renderDevices, settings: renderSettings };
  (views[S.view] || renderOverview)();
}

// Accessibility pass over whatever a view just rendered: label the controls the views
// build as plain markup, and make clickable rows reachable and operable from the keyboard.
let uid = 0, a11yTimer = 0;
const ROW_SEL = ".slot, .bank, .wt, .preset, .akwf-groups > div, .akwf-files > div, td.cell";
const SCROLL_SEL = ".tbl-scroll, .api-list, .banks, .monitor, .wt-grid, .akwf-groups, .akwf-files";
function a11y(root) {
  $$(".field", root).forEach((f) => {
    const lab = f.querySelector(":scope > label:not([for])"), ctl = f.querySelector("input, select, textarea");
    if (lab && ctl) { if (!ctl.id) ctl.id = "f" + (++uid); lab.htmlFor = ctl.id; }
  });
  $$("label.switch input:not([aria-label])", root).forEach((i) => {
    let name = "";
    if (i.dataset.svc) { const td = i.closest("tr") && i.closest("tr").firstElementChild; name = td ? td.textContent.trim() : ""; }
    if (!name) { const h = i.closest(".card-head") && i.closest(".card-head").querySelector("h3"); name = h ? h.textContent.trim() : ""; }
    if (!name) { const fl = i.closest(".field") && i.closest(".field").querySelector("label:not(.switch)"); name = fl ? fl.textContent.trim() : ""; }
    if (!name) { const nx = i.closest("label.switch").nextElementSibling; name = nx ? nx.textContent.trim() : ""; }
    if (!name) { const pv = i.closest("label.switch").previousElementSibling; name = pv ? pv.textContent.trim() : ""; }
    if (name) i.setAttribute("aria-label", name);
  });
  // Rows that hold their own button (a play button) stay plain focusable rows; the rest act as buttons.
  $$(ROW_SEL, root).forEach((el) => { if (!el.hasAttribute("tabindex")) { el.setAttribute("tabindex", "0"); if (!el.querySelector("button, input, select, a")) el.setAttribute("role", "button"); } });
  $$(SCROLL_SEL + ", .card", root).forEach((el) => {
    if (el.hasAttribute("tabindex") || (el.scrollHeight <= el.clientHeight + 1 && el.scrollWidth <= el.clientWidth + 1)) return;
    el.setAttribute("tabindex", "0"); // keyboard users can scroll it with the arrow keys
  });
  $$("[title]:is(input, select, button, textarea):not([aria-label])", root).forEach((el) => { if (!(el.labels && el.labels.length) && !el.textContent.trim()) el.setAttribute("aria-label", el.title); });
  $$("input:not([aria-label]):not([type=hidden])", root).forEach((el) => { if (!(el.labels && el.labels.length) && el.title) el.setAttribute("aria-label", el.title); });
  $$(".param", root).forEach((p) => { const i = p.querySelector("input:not([aria-label])"), l = p.querySelector(".lbl"); if (i && l) i.setAttribute("aria-label", l.textContent.trim()); });
  $$(".card-head h3", root).forEach((h) => h.setAttribute("aria-level", "2")); // sections under the page's h1
  $$("th:empty", root).forEach((th) => { th.innerHTML = `<span class="sr">${th.cellIndex === th.parentElement.children.length - 1 ? "Actions" : "Status"}</span>`; });
  $$("select:not([aria-label]):not([id])", root).forEach((el) => { const t = el.title || el.closest("td") && el.closest("table").querySelectorAll("th")[el.closest("td").cellIndex]; if (t) el.setAttribute("aria-label", typeof t === "string" ? t : t.textContent.trim()); });
  $$("input.input:not([aria-label]):not([id])", root).forEach((el) => { const th = el.closest("td") && el.closest("table").querySelectorAll("th")[el.closest("td").cellIndex]; if (th) el.setAttribute("aria-label", th.textContent.trim()); });
}
new MutationObserver(() => { if (!a11yTimer) a11yTimer = setTimeout(() => { a11yTimer = 0; a11y($("#main")); }, 120); }).observe($("#main"), { childList: true, subtree: true });
$("#main").addEventListener("keydown", (e) => {
  const el = e.target;
  if ((e.key === "Enter" || e.key === " ") && el.matches && el.matches(ROW_SEL)) { e.preventDefault(); el.click(); }
});

// Scene TOML uses [[slot]] / [[route]]; the UI works with plural names.
function norm(sc) { sc.slots = sc.slot || []; sc.routes = sc.route || []; return sc; }

async function refreshScene() {
  try { const r = await get("/scene"); S.scene = norm(r.scene); S.dirty = r.dirty; S.inputs = r.midi_inputs; S.devices = r.devices || []; } catch (e) { fail(e); }
}

function head(title, sub, right = "") {
  return `<div class="page-head"><div><h1>${esc(title)}</h1>${sub ? `<div class="sub">${sub}</div>` : ""}</div><div class="spacer"></div>${right}</div>`;
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
  const usb = (s.usb || []).map((u) => `<tr><td>${esc(u.product || u.id)}</td><td class="muted">${esc(u.vendor)}</td><td class="mono small">${u.id}</td><td>${u.midi ? `<span class="badge ok">MIDI</span>` : u.audio ? `<span class="badge">audio</span>` : ""}</td></tr>`).join("") || `<tr><td class="empty" colspan="4">No USB devices connected.</td></tr>`;
  const inputs = (S.inputs || []).filter((i) => i.slot !== 9).map((i) => {
    const who = i.device ? `<span class="badge ok">${esc(i.device)}</span> ` : "";
    const ident = i.identity ? `<div class="mono small muted">${esc(i.identity)}${i.rule ? " · matched by " + esc(i.rule) : ""}</div>` : "";
    if (i.slot == null) return `<tr class="muted"><td><span class="act"></span>${who}${esc(i.label)}${ident}<div class="small">${esc(i.skipped)}</div></td><td class="mono small">not connected</td><td class="num">—</td></tr>`;
    return `<tr><td><span class="act" data-port="${i.slot}"></span>${who}${esc(i.label)}${ident}</td><td class="mono small muted">${i.slot === 0 ? `${(S.info && S.info.synth) || "Trillium Synth"}:midi_in` : i.slot === 8 ? "web keyboard" : "in " + i.slot}</td><td class="num">${i.events}</td></tr>`;
  }).join("");
  const ev = (s.events || []).map((e) => `<div><span class="t">${e.time}</span><span>${e.level}</span><span></span><span>${esc(e.text)}</span></div>`).join("");
  const rec = s.recording;
  $("#main").innerHTML = head("Overview", `${esc((S.info && S.info.device) || "Trillium Base")} · ${esc(s.hostname)}.local · ${esc((s.addresses || []).filter((a) => !a.includes(":")).join(" · "))} · port ${s.port}`) + `
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
    if (sEl) sEl.innerHTML = `<table class="storage"><thead><tr><th>Device</th><th class="num">Size</th><th class="num">Used</th><th class="num">Free</th><th>Use</th><th>Filesystem · mount</th><th class="num">Temp</th></tr></thead><tbody>${rows.join("") || `<tr><td class="empty" colspan="7">No disks found.</td></tr>`}</tbody></table>`;
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
// Styled replacements for the browser's confirm() and prompt(): true/false and text/null.
const ask = async (title, body, ok = "OK") => (await dialog({ title, body: `<p>${body}</p>`, ok })) !== null;
function askText(title, body, value = "", ok = "OK") {
  return new Promise((resolve) => {
    const d = document.createElement("dialog");
    d.className = "dlg";
    d.innerHTML = `<h3>${esc(title)}</h3><form method="dialog"><div class="dlg-body"><p>${body}</p></div><input class="input" id="dlg-text" autocomplete="off" aria-label="${esc(title)}" value="${esc(value)}">
      <div class="row" style="justify-content:flex-end;margin-top:14px"><button class="btn ghost" type="button" data-x="cancel">Cancel</button><button class="btn primary" type="submit" data-x="ok">${esc(ok)}</button></div></form>`;
    document.body.appendChild(d);
    const inp = $("#dlg-text", d);
    const done = (v) => { d.close(); d.remove(); resolve(v); };
    $("form", d).addEventListener("submit", (e) => { e.preventDefault(); const v = inp.value.trim(); done(v || null); });
    $('[data-x="cancel"]', d).addEventListener("click", () => done(null));
    d.addEventListener("cancel", () => done(null));
    d.showModal(); inp.focus(); inp.select();
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
        <span class="row tight muted small"><span>Enabled</span><label class="switch"><input type="checkbox" id="slot-en" aria-label="Sound enabled" ${snd.enabled ? "checked" : ""}><span></span></label></span>
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
  else if (!sm.available) smHtml = `<div class="risk">No sample banks installed at <code>${esc(sm.root)}</code>. On the device run <code>sh scripts/fetch-samples.sh</code> (CC0 FreePats banks and the VCSL drum kit, about 120 MiB).</div>`;
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
    <td><label class="switch"><input type="checkbox" data-k="enabled" aria-label="Route enabled" ${r.enabled ? "checked" : ""}><span></span></label></td>
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
    if (S.dirty && !(await ask("Load this scene?", "The active scene has unsaved changes. Loading another scene discards them.", "Load anyway"))) return;
    try { const r = await post(`/scenes/${encodeURIComponent(b.dataset.load)}/load`); S.scene = norm(r.scene); S.dirty = r.dirty; toast(`Loaded ${b.dataset.load}`); renderScenes(); } catch (e) { fail(e); }
  }));
  $$("[data-dup]").forEach((b) => b.addEventListener("click", async () => {
    const to = await askText("Duplicate scene", `Name for the copy of <b>${esc(b.dataset.dup)}</b>.`, b.dataset.dup + " copy", "Duplicate"); if (!to) return;
    try { await post(`/scenes/${encodeURIComponent(b.dataset.dup)}/duplicate`, { to }); renderScenes(); } catch (e) { fail(e); }
  }));
  $$("[data-rm]").forEach((b) => b.addEventListener("click", async () => {
    if (!(await ask("Delete scene?", `Delete <b>${esc(b.dataset.rm)}</b>? A .bak copy of the last save stays on the device.`, "Delete"))) return;
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
// ------------------------------------------------------------------ signals: patch panel
// Sources (outputs) on the left, destinations (inputs) on the right. Each side is a grid:
// device | port | stereo bracket | jack, mirrored on the right. Cables run jack centre to
// jack centre. Jacks: filled = connected, ring = free, dimmed = cannot take the cable
// being dragged. Audio jacks are warm white, MIDI jacks ash. Orange only marks the cable
// or jack under the pointer or selected.
const SP = { kind: "all", hideFree: false, monitors: false, q: "", sel: null, pick: null, undo: [], drag: null, graph: null, gkey: "", mixer: null, meters: {}, lvl: {}, timer: null, tick: 0 };
const SYN = () => (S.info && S.info.synth) || "Trillium Synth";

function pairKey(short) {
  let m = short.match(/^(.*?)(FL|FR)$/); if (m) return [m[1], m[2] === "FL" ? 0 : 1];
  m = short.match(/^(.*?[_-])(l|r|L|R|left|right)$/); if (m) return [m[1], /^l/i.test(m[2]) ? 0 : 1];
  m = short.match(/^(.*?)(\d+)$/); if (m) { const n = +m[2]; return [m[1] + "#" + Math.floor((n - 1) / 2), (n - 1) % 2]; }
  return null;
}
// Rows of a node: [{p, pair: "a"|"b"|null}], pairs only between neighbours of one kind.
function rowsOf(ps) {
  const rows = ps.map((p) => ({ p, pair: null }));
  for (let i = 0; i + 1 < rows.length; i++) {
    const a = rows[i].p, b = rows[i + 1].p;
    if (rows[i].pair || a.kind !== "audio" || b.kind !== "audio") continue;
    const ka = pairKey(a.short), kb = pairKey(b.short);
    if (ka && kb && ka[0] === kb[0] && ka[1] === 0 && kb[1] === 1) { rows[i].pair = "a"; rows[i + 1].pair = "b"; i++; }
  }
  return rows;
}
const mixNode = (client) => SP.mixer && SP.mixer.nodes.find((n) => n.label === client);

function sigModel() {
  const all = SP.graph.ports.filter((p) => p.kind === "audio" || p.kind === "midi");
  const names = new Set(all.map((p) => p.name));
  const links = all.filter((p) => p.dir === "out").flatMap((p) => p.connections.filter((c) => names.has(c)).map((c) => ({ a: p.name, b: c, kind: p.kind })));
  const linked = new Set(links.flatMap((l) => [l.a, l.b]));
  const q = SP.q.trim().toLowerCase();
  const show = (p) => (SP.kind === "all" || p.kind === SP.kind)
    && (SP.monitors || !(p.dir === "out" && p.short.startsWith("monitor_")))
    && (!SP.hideFree || linked.has(p.name))
    && (!q || p.client.toLowerCase().includes(q) || p.short.toLowerCase().includes(q));
  const side = (dir) => {
    const m = new Map();
    for (const p of all.filter((x) => x.dir === dir && show(x))) { if (!m.has(p.client)) m.set(p.client, []); m.get(p.client).push(p); }
    const rank = (c, ps) => {
      const n = mixNode(c);
      if (c === SYN()) return dir === "out" ? 0 : 1;
      if (n && SP.mixer && n.name === SP.mixer.dac_sink) return dir === "in" ? 0 : 5;
      if (ps.every((p) => p.short.startsWith("monitor_"))) return 6;
      if (n && n.airplay) return 4;
      if (ps.some((p) => p.physical)) return 3;
      return 2;
    };
    return Array.from(m.entries()).map(([c, ps]) => ({ c, ps, r: rank(c, ps) })).sort((x, y) => x.r - y.r || x.c.localeCompare(y.c));
  };
  const L = side("out"), R = side("in");
  const vis = new Set([...L, ...R].flatMap((g) => g.ps.map((p) => p.name)));
  return { links: links.filter((l) => vis.has(l.a) && vis.has(l.b)), allLinks: links, linked, L, R };
}

function devTags(c, ps, dir) {
  const n = mixNode(c), t = [];
  if (c === SYN()) { t.push("this synth"); if (ps.every((p) => p.kind === "midi")) t.push("MIDI in"); return t; }
  if (n && SP.mixer && n.name === SP.mixer.dac_sink) t.push("DAC");
  if (n && n.airplay) t.push("AirPlay · output only");
  else if (n && n.role === "sink") t.push("output");
  else if (n && n.role === "source") t.push("capture");
  else if (ps.every((p) => p.short.startsWith("monitor_"))) t.push("monitor");
  else if (ps.some((p) => p.physical)) t.push("hardware");
  else if (n && n.role === "jack") t.push("JACK app");
  if (ps.every((p) => p.kind === "midi")) t.push("MIDI");
  return t;
}

// Level strip: meter (Amber), fader, mute. Synth: its master (capped at -6 dBFS) and fade.
function stripHtml(c, ps) {
  if (!ps.some((p) => p.kind === "audio")) return "";
  const id = esc(c);
  const meter = `<span class="mtr" data-mtr="${id}"><i><b></b></i><i><b></b></i></span>`;
  if (c === SYN()) {
    const db = S.status ? S.status.master_db : -18, on = S.status ? S.status.running : true;
    return `<div class="strip">${meter}<input type="range" class="mini" data-syn-fader min="-60" max="-6" step="0.5" value="${db}" title="Synth master (capped at −6 dBFS)"><button class="mute ${on ? "" : "on"}" data-syn-mute title="Fade the synth out / in">M</button><span class="sv mono">${fmtDb(db).replace(" dB", "")}</span></div>`;
  }
  const n = mixNode(c);
  if (!n || n.volume == null) return `<div class="strip">${meter}<span class="muted tiny">${n && n.role === "jack" ? "level set in the app" : "no volume control"}</span></div>`;
  const v = Math.round(n.volume * 100), db = n.volume > 0 ? 60 * Math.log10(n.volume) : -120;
  return `<div class="strip">${meter}<input type="range" class="mini" data-vol="${n.id}" data-name="${esc(n.name)}" min="0" max="${Math.round(SP.mixer.cap * 100)}" value="${v}" title="Volume (WirePlumber scale; capped at 100%, never boosts)"><button class="mute ${n.mute ? "on" : ""}" data-mute="${n.id}" data-name="${esc(n.name)}" title="Mute">M</button><span class="sv mono">${n.volume > 0 ? db.toFixed(1) : "−∞"}</span></div>`;
}

function groupHtml(g, dir, model) {
  const rows = rowsOf(g.ps), n = rows.length, src = dir === "out";
  const strip = stripHtml(g.c, g.ps);
  const tags = devTags(g.c, g.ps, dir).map((t) => `<span class="dtag">${esc(t)}</span>`).join("");
  const dev = `<div class="dev" style="grid-row:1 / span ${Math.max(n, strip && n < 2 ? 2 : n)}"><div class="dhead"><span class="dn" title="${esc(g.c)}">${esc(g.c)}</span><span class="dtags">${tags}</span></div>${strip}</div>`;
  const cells = rows.map((r, i) => {
    const p = r.p, on = model.linked.has(p.name), row = i + 1;
    const jk = `<span class="jkc" style="grid-row:${row}"><span class="jk ${p.kind} ${on ? "on" : ""} ${SP.pick === p.name ? "hot" : ""}" data-port="${esc(p.name)}" data-dir="${dir}" data-kind="${p.kind}" title="${esc(p.name)} · ${on ? "connected" : "free"}"></span></span>`;
    const pn = `<span class="pn ${i % 2 ? "odd" : ""}" style="grid-row:${row}" data-port="${esc(p.name)}" data-dir="${dir}" data-kind="${p.kind}">${esc(p.short)}</span>`;
    const br = r.pair === "a" ? `<span class="brk" style="grid-row:${row} / span 2" data-pair="${esc(p.name)}|${esc(rows[i + 1].p.name)}" data-dir="${dir}" title="Drag to patch the stereo pair"></span>` : r.pair === "b" ? "" : `<span class="brc" style="grid-row:${row}"></span>`;
    return src ? pn + br + jk : jk + br + pn;
  }).join("");
  const filler = strip && n < 2 ? `<span class="pn" style="grid-row:2"></span>` : "";
  return `<div class="pg ${src ? "src" : "dst"}">${dev}${cells}${filler}</div>`;
}

async function renderSignals() {
  try {
    const [g, m] = await Promise.all([get("/graph"), get("/mixer").catch(() => SP.mixer || { nodes: [], cap: 1 })]);
    SP.graph = g; SP.mixer = m; SP.gkey = JSON.stringify(g.ports) + JSON.stringify(m.nodes);
  } catch (e) { fail(e); return; }
  renderSignalsNoFetch();
  if (!SP.timer) SP.timer = setInterval(sigTick, 125);
}

// Renders from the data in hand (filters, patching, polling); keeps the scroll position.
function renderSignalsNoFetch() {
  if (!SP.graph) return;
  const main = $("#main"), sc = main.scrollTop;
  const model = sigModel();
  const tool = `<div class="pp-tools">
    <div class="seg" id="sp-kind">${[["all", "All"], ["audio", "Audio"], ["midi", "MIDI"]].map(([k, l]) => `<button data-k="${k}" class="${SP.kind === k ? "on" : ""}">${l}</button>`).join("")}</div>
    <label class="row tight small"><input type="checkbox" id="sp-free" ${SP.hideFree ? "checked" : ""}> Hide unconnected</label>
    <label class="row tight small"><input type="checkbox" id="sp-mon" ${SP.monitors ? "checked" : ""}> Monitors</label>
    <input class="input small" id="sp-q" placeholder="Search devices or ports" value="${esc(SP.q)}" style="width:200px">
    <span class="spacer"></span>
    <span class="legend"><span class="jk audio on"></span>connected <span class="jk audio"></span>free <span class="jk audio dimj"></span>can't take this cable <span class="sep"></span><span class="jk audio on"></span>audio <span class="jk midi on"></span>MIDI</span>
    <span class="mono muted small">${model.links.length} cable${model.links.length === 1 ? "" : "s"}</span>
    <button class="btn ghost small" id="sp-undo" ${SP.undo.length ? "" : "disabled"} title="Undo (⌘Z / Ctrl+Z)">Undo</button>
  </div>`;
  const empty = (t) => `<div class="pp-empty muted small">${t}</div>`;
  $("#main").innerHTML = head("Signals", `Patch panel: sources on the left, destinations on the right (PipeWire via JACK, ${esc(SP.graph.backend)}). Drag from a jack to a jack to connect; drag a stereo bracket to patch both sides; click a cable to select it, Delete removes it.`, `<button class="btn ghost" id="sig-refresh">Refresh</button>`) + `
  <div class="card pp-card">${tool}
    <div class="pp" id="pp">
      <div class="pp-side src"><div class="pp-h"><span>Source</span><span class="r">Output</span><span></span><span></span></div>${model.L.map((g) => groupHtml(g, "out", model)).join("") || empty("No sources match.")}</div>
      <div class="pp-gut"></div>
      <div class="pp-side dst"><div class="pp-h"><span></span><span></span><span>Input</span><span class="r">Destination</span></div>${model.R.map((g) => groupHtml(g, "in", model)).join("") || empty("No destinations match.")}</div>
      <svg id="pp-svg"></svg>
    </div>
    <div class="muted small pp-foot">Solid jack: has at least one cable. Ring: free. Dimmed while dragging: wrong type or direction. Hardware MIDI sources are connected to <span class="mono">${esc(SYN())}:in_N</span> automatically; other apps can use <span class="mono">${esc(SYN())}:midi_in</span>. AirPlay speakers are outputs only and appear while the AirPlay sender is on. Volumes never go above 100% (no boost), so the synth's −6 dBFS cap and limiter still bound the DAC.</div>
  </div>`;
  SP.model = model;
  main.scrollTop = sc;
  drawCables();
  bindSignals();
  paintMeters();
}

// Polls: meters at 8 Hz, graph every 3 s (re-rendered only when it changed), mixer every 6 s.
async function sigTick() {
  if (S.view !== "signals") { clearInterval(SP.timer); SP.timer = null; return; }
  SP.tick++;
  try { SP.meters = await get("/meters"); } catch (e) { /* ignore */ }
  paintMeters();
  if (SP.tick % 24 === 0 && !SP.drag && !document.querySelector(".pp input.mini:active") && document.activeElement !== $("#sp-q")) {
    try {
      const g = await get("/graph"), m = SP.tick % 48 === 0 ? await get("/mixer") : SP.mixer;
      const k = JSON.stringify(g.ports) + JSON.stringify(m.nodes);
      if (k !== SP.gkey) { SP.graph = g; SP.mixer = m; SP.gkey = k; renderSignalsNoFetch(); }
    } catch (e) { /* offline */ }
  }
}
function paintMeters() {
  const lv = {};
  const dbOf = (c) => {
    if (c === SYN() && S.status) return S.status.peak_db;
    return SP.meters[c];
  };
  // sinks without their own meter show what their sources send
  const model = SP.model; if (!model) return;
  const clientOf = (port) => port.split(":")[0];
  for (const g of [...model.L, ...model.R]) {
    let v = dbOf(g.c);
    if (!v) {
      const srcs = model.allLinks.filter((l) => clientOf(l.b) === g.c).map((l) => dbOf(clientOf(l.a))).filter(Boolean);
      if (srcs.length) v = [Math.max(...srcs.map((s) => s[0])), Math.max(...srcs.map((s) => s[s.length > 1 ? 1 : 0]))];
    }
    lv[g.c] = v || [-120, -120];
  }
  $$("[data-mtr]").forEach((el) => {
    const c = el.dataset.mtr, v = lv[c] || [-120, -120], old = SP.lvl[c] || [-120, -120];
    const now = [0, 1].map((i) => Math.max(v[Math.min(i, v.length - 1)] ?? -120, old[i] - 2.5));
    SP.lvl[c] = now;
    el.querySelectorAll("b").forEach((b, i) => { b.style.width = Math.max(0, Math.min(100, (now[i] + 60) / 60 * 100)) + "%"; });
    el.title = `${c}: ${now.map((x) => (x <= -119 ? "−∞" : x.toFixed(0))).join(" / ")} dBFS`;
  });
}

function jackPos(name, dir) {
  const box = $("#pp"); if (!box) return null;
  const el = box.querySelector(`.jk[data-dir="${dir}"][data-port="${CSS.escape(name)}"]`); if (!el) return null;
  const R = box.getBoundingClientRect(), r = el.getBoundingClientRect();
  return { x: r.left + r.width / 2 - R.left, y: r.top + r.height / 2 - R.top };
}
const bez = (a, b) => { const d = Math.max(50, Math.abs(b.x - a.x) * 0.5); return `M${a.x},${a.y} C${a.x + d},${a.y} ${b.x - d},${b.y} ${b.x},${b.y}`; };

function drawCables() {
  const svg = $("#pp-svg"), box = $("#pp"); if (!svg || !box) return;
  svg.setAttribute("width", box.clientWidth); svg.setAttribute("height", box.clientHeight);
  const out = [];
  for (const l of SP.model.links) {
    const a = jackPos(l.a, "out"), b = jackPos(l.b, "in"); if (!a || !b) continue;
    const key = l.a + "|" + l.b, d = bez(a, b), sel = SP.sel === key;
    out.push(`<g class="cg" data-key="${esc(key)}"><path class="cb ${l.kind} ${sel ? "sel" : ""}" d="${d}"/><path class="hit" d="${d}"/>${sel ? `<g class="del" data-del="${esc(key)}" transform="translate(${(a.x + b.x) / 2},${(a.y + b.y) / 2})"><circle r="8"/><path d="M-3,-3 L3,3 M3,-3 L-3,3"/></g>` : ""}</g>`);
  }
  svg.innerHTML = out.join("") + `<path id="pp-temp" class="cb temp" d=""/><path id="pp-temp2" class="cb temp" d=""/>`;
  $$("#pp-svg .cg").forEach((g) => {
    const [a, b] = g.dataset.key.split("|");
    g.addEventListener("mouseenter", () => { g.querySelector(".cb").classList.add("hot"); hotJacks([a, b], true); svg.appendChild(g); });
    g.addEventListener("mouseleave", () => { g.querySelector(".cb").classList.remove("hot"); hotJacks([a, b], false); });
    g.querySelector(".hit").addEventListener("click", (e) => { e.stopPropagation(); SP.sel = SP.sel === g.dataset.key ? null : g.dataset.key; drawCables(); });
  });
  $$("#pp-svg [data-del]").forEach((d) => d.addEventListener("click", (e) => { e.stopPropagation(); const [a, b] = d.dataset.del.split("|"); patch("disconnect", [[a, b]]); }));
}
function hotJacks(names, on) { names.forEach((n) => $$(`#pp .jk[data-port="${CSS.escape(n)}"]`).forEach((j) => j.classList.toggle("hot", on))); }

// Connect or disconnect a set of links; recorded for undo unless it is the undo itself.
async function patch(op, pairs, record = true) {
  const done = [];
  for (const [from, to] of pairs) {
    try { await post(op === "connect" ? "/graph/connect" : "/graph/disconnect", { from, to }); done.push([from, to]); }
    catch (e) { if (!/Exist/i.test(e.message)) fail(e); }
  }
  if (done.length && record) { SP.undo.push({ op, pairs: done }); if (SP.undo.length > 50) SP.undo.shift(); }
  if (done.length) toast(`${op === "connect" ? "Connected" : "Disconnected"} ${done.map(([a, b]) => `${a.split(":").pop()} → ${b}`).join(", ")}${record ? " · ⌘Z to undo" : ""}`);
  SP.sel = null;
  try { SP.graph = await get("/graph"); } catch (e) {}
  renderSignalsNoFetch();
}
function undoPatch() {
  const u = SP.undo.pop(); if (!u) return toast("Nothing to undo");
  patch(u.op === "connect" ? "disconnect" : "connect", u.pairs, false);
}

function bindSignals() {
  $("#sig-refresh").addEventListener("click", renderSignals);
  $$("#sp-kind button").forEach((b) => b.addEventListener("click", () => { SP.kind = b.dataset.k; renderSignalsNoFetch(); }));
  $("#sp-free").addEventListener("change", (e) => { SP.hideFree = e.target.checked; renderSignalsNoFetch(); });
  $("#sp-mon").addEventListener("change", (e) => { SP.monitors = e.target.checked; renderSignalsNoFetch(); });
  $("#sp-q").addEventListener("input", (e) => { SP.q = e.target.value; debounce("spq", () => { renderSignalsNoFetch(); const q = $("#sp-q"); q.focus(); q.setSelectionRange(q.value.length, q.value.length); }, 180); });
  $("#sp-undo").addEventListener("click", undoPatch);
  $("#pp").addEventListener("click", (e) => { if (!e.target.closest(".jk,.pn,.brk,.cg") && SP.sel) { SP.sel = null; drawCables(); } });
  // levels
  $$("[data-vol]").forEach((f) => f.addEventListener("input", () => {
    const v = +f.value / 100, sv = f.parentElement.querySelector(".sv");
    sv.textContent = v > 0 ? (60 * Math.log10(v)).toFixed(1) : "−∞";
    debounce("vol" + f.dataset.vol, () => post(`/mixer/${f.dataset.vol}`, { name: f.dataset.name, volume: v }).then((n) => { const x = mixNode(n.label); if (x) Object.assign(x, n); }).catch(fail), 90);
  }));
  $$("[data-mute]").forEach((b) => b.addEventListener("click", async () => {
    const on = !b.classList.contains("on");
    try { const n = await post(`/mixer/${b.dataset.mute}`, { name: b.dataset.name, mute: on }); b.classList.toggle("on", !!n.mute); const x = mixNode(n.label); if (x) Object.assign(x, n); } catch (e) { fail(e); }
  }));
  const sf = $("[data-syn-fader]");
  if (sf) sf.addEventListener("input", () => {
    const db = +sf.value; sf.parentElement.querySelector(".sv").textContent = fmtDb(db).replace(" dB", "");
    $("#master").value = db; $("#master-v").textContent = fmtDb(db);
    debounce("master", () => post("/master", { db }).catch(fail), 60);
  });
  const sm = $("[data-syn-mute]");
  if (sm) sm.addEventListener("click", async () => { const run = sm.classList.contains("on"); try { await post("/engine", { run }); sm.classList.toggle("on", !run); } catch (e) { fail(e); } });
  // drag to connect (jack, port name or stereo bracket); click-click also works
  $$("#pp .jk, #pp .pn[data-port], #pp .brk").forEach((el) => el.addEventListener("pointerdown", startDrag));
}

function targetsFor(d) {
  const want = d.dir === "out" ? "in" : "out";
  $$("#pp .jk").forEach((j) => {
    const ok = j.dataset.dir === want && j.dataset.kind === d.kind;
    j.classList.toggle("tgt", ok); j.classList.toggle("dim", !ok && j.dataset.port !== d.ports[0]);
  });
}
function startDrag(e) {
  if (e.button !== 0) return;
  const el = e.currentTarget, br = el.classList.contains("brk");
  const ports = br ? el.dataset.pair.split("|") : [el.dataset.port];
  const dir = el.dataset.dir, j = $(`#pp .jk[data-dir="${dir}"][data-port="${CSS.escape(ports[0])}"]`); if (!j) return;
  let pair = br ? ports : null;
  if (!pair && e.shiftKey) { const b = el.closest(".pg").querySelector(`.brk[data-pair*="${CSS.escape(ports[0])}"]`); if (b) pair = b.dataset.pair.split("|"); }
  const d = { ports: pair || ports, dir, kind: j.dataset.kind, x0: e.clientX, y0: e.clientY, moved: false, pair: !!pair };
  SP.drag = d; e.preventDefault();
  const box = $("#pp"), temp = $("#pp-temp"), temp2 = $("#pp-temp2");
  const from = jackPos(d.ports[0], dir), from2 = d.pair ? jackPos(d.ports[1], dir) : null;
  const move = (ev) => {
    if (!d.moved && Math.hypot(ev.clientX - d.x0, ev.clientY - d.y0) < 4) return;
    if (!d.moved) { d.moved = true; box.classList.add("dragging"); targetsFor(d); }
    const R = box.getBoundingClientRect(), p = { x: ev.clientX - R.left, y: ev.clientY - R.top };
    temp.setAttribute("d", dir === "out" ? bez(from, p) : bez(p, from));
    if (from2) { const p2 = { x: p.x, y: p.y + (from2.y - from.y) }; temp2.setAttribute("d", dir === "out" ? bez(from2, p2) : bez(p2, from2)); }
    $$("#pp .jk.hot2").forEach((x) => x.classList.remove("hot2"));
    const t = dropTarget(ev, d); if (t) t.classList.add("hot2");
  };
  const up = (ev) => {
    window.removeEventListener("pointermove", move); window.removeEventListener("pointerup", up);
    box.classList.remove("dragging"); temp.setAttribute("d", ""); temp2.setAttribute("d", ""); SP.drag = null;
    $$("#pp .jk").forEach((x) => x.classList.remove("tgt", "dim", "hot2"));
    if (!d.moved) return clickPort(d.ports[0], dir, d.kind);
    const t = dropTarget(ev, d); if (!t) return;
    connectFrom(d, t.dataset.port, t.closest(".pg"));
  };
  window.addEventListener("pointermove", move); window.addEventListener("pointerup", up);
}
function dropTarget(ev, d) {
  const want = d.dir === "out" ? "in" : "out";
  const el = document.elementFromPoint(ev.clientX, ev.clientY); if (!el) return null;
  const hit = el.closest(`.jk, .pn[data-port], .brk`); if (!hit || hit.dataset.dir !== want) return null;
  const port = hit.classList.contains("brk") ? hit.dataset.pair.split("|")[0] : hit.dataset.port;
  const j = $(`#pp .jk[data-dir="${want}"][data-port="${CSS.escape(port)}"]`);
  return j && j.dataset.kind === d.kind ? j : null;
}
function connectFrom(d, target, group) {
  let tgts = [target];
  if (d.pair) {
    const b = group && Array.from(group.querySelectorAll(".brk")).find((x) => x.dataset.pair.split("|").includes(target));
    tgts = b ? b.dataset.pair.split("|") : [target];
    if (tgts.length < 2) { // mono target: both sides of the pair go to it
      tgts = [target, target];
    }
  }
  const pairs = d.ports.map((p, i) => d.dir === "out" ? [p, tgts[Math.min(i, tgts.length - 1)]] : [tgts[Math.min(i, tgts.length - 1)], p]);
  const fresh = pairs.filter(([a, b]) => !SP.model.allLinks.some((l) => l.a === a && l.b === b));
  if (!fresh.length) return toast("Already connected");
  patch("connect", fresh);
}
function clickPort(port, dir, kind) {
  if (!SP.pick || SP.pick === port) { SP.pick = SP.pick === port ? null : port; SP.pickDir = dir; SP.pickKind = kind; hotJacks([port], !!SP.pick); return; }
  if (SP.pickDir === dir || SP.pickKind !== kind) { hotJacks([SP.pick], false); SP.pick = port; SP.pickDir = dir; SP.pickKind = kind; hotJacks([port], true); return; }
  const [a, b] = dir === "in" ? [SP.pick, port] : [port, SP.pick];
  SP.pick = null;
  const exists = SP.model.allLinks.some((l) => l.a === a && l.b === b);
  patch(exists ? "disconnect" : "connect", [[a, b]]);
}
document.addEventListener("keydown", (e) => {
  if (S.view !== "signals" || /INPUT|TEXTAREA|SELECT/.test(document.activeElement.tagName)) return;
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "z") { e.preventDefault(); undoPatch(); }
  else if ((e.key === "Delete" || e.key === "Backspace") && SP.sel) { e.preventDefault(); const [a, b] = SP.sel.split("|"); patch("disconnect", [[a, b]]); }
  else if (e.key === "Escape") { SP.sel = null; SP.pick = null; renderSignalsNoFetch(); }
});
window.addEventListener("resize", () => { if (S.view === "signals" && SP.model) drawCables(); });

// ------------------------------------------------------------------ settings
// ------------------------------------------------------------------ devices
// USB and Bluetooth devices, the audio input strip, and Trillium Zero profiles.
// Re-applying a stored profile always needs an explicit click and a confirm.
async function renderDevices() {
  let d, inp;
  try { [d, inp] = await Promise.all([get("/devices"), get("/input")]); } catch (e) { fail(e); return; }
  if (S.view !== "devices") return;
  const usb = (d.usb || []).map((u) => {
    const canIn = u.audio && !/trill/i.test(u.product || "");
    const glob = inputGlobFor(u);
    const on = glob && inp.source && glob === inp.source;
    const btn = canIn ? `<button class="btn small ghost" data-as-input="${esc(glob)}">${on ? "Current input" : "Use as input"}</button>` : "";
    return `<tr><td class="mono">${esc(u.id)}</td><td>${esc(u.product)}</td><td class="muted">${esc(u.vendor)}</td><td>${u.midi ? '<span class="badge ok">MIDI</span> ' : ""}${u.audio ? '<span class="badge ok">audio</span>' : ""}</td><td>${btn}</td></tr>`;
  }).join("");
  const ble = (d.ble_connected || []).map((b) => `<span class="badge ok">${esc(b)}</span>`).join(" ") || '<span class="muted">none connected</span>';
  const zeros = (d.zeros || []).map(zeroCard).join("") || `<div class="card"><div class="card-head"><h3>Trillium Zero</h3><span class="spacer"></span><span class="badge off">not connected</span></div></div>`;
  const src = inp.connected.length ? inp.connected.map(esc).join(", ") : "not connected";
  $("#main").innerHTML = head("Devices", "What is plugged in, the audio input strip, and per-device Trillium Zero profiles") + `
    <div class="card">
      <div class="card-head"><h3>Audio input strip</h3><span class="spacer"></span><span class="badge ${inp.connected.length ? "ok" : "off"}">${inp.connected.length ? "connected" : "idle"}</span></div>
      <div class="row">
        <div class="field" style="flex:1"><label>Source (port name, * wildcards)</label><input class="input mono" id="in-src" value="${esc(inp.source)}" list="in-cands"><datalist id="in-cands">${(inp.candidates || []).map((c) => `<option value="${esc(c)}">`).join("")}</datalist></div>
        <div class="field"><label>Gain (dB)</label><input class="input mono" id="in-gain" type="number" min="-60" max="24" step="0.5" value="${inp.gain_db}" style="width:90px"></div>
        <div class="field"><label>Record</label><label class="switch"><input type="checkbox" id="in-rec" ${inp.record ? "checked" : ""}><span></span></label></div>
        <div class="field"><label>Monitor to DAC</label><label class="switch"><input type="checkbox" id="in-mon" ${inp.monitor ? "checked" : ""}><span></span></label></div>
        <div class="meters" style="align-self:center"><div class="meter"><div class="fill" id="in-l"></div></div><div class="meter"><div class="fill" id="in-r"></div></div></div>
      </div>
      <div class="muted" style="margin-top:6px">Linked: <span class="mono">${src}</span> → <span class="mono">${esc((inp.ports || []).join(", "))}</span></div>
      <div class="risk ${inp.monitor ? "caution" : ""}" style="margin-top:8px">${inp.monitor ? "<b>!</b> Monitoring is on: the input plays through the DAC. A microphone near the speakers can feed back." : "Monitoring is off: the input goes to recordings only, never to the DAC."} There is no synth effect input yet; effects come later.</div>
      <div style="margin-top:10px"><button class="btn small" id="in-save">Save input</button></div>
    </div>
    ${zeros}
    <div class="card">
      <div class="card-head"><h3>USB devices</h3></div>
      <table><tbody>${usb || '<tr><td class="empty" colspan="4">No USB devices connected.</td></tr>'}</tbody></table>
      <div class="lbl" style="margin:14px 0 4px">Bluetooth</div><div>${ble}</div>
    </div>`;
  $("#in-save").onclick = async () => {
    const mon = $("#in-mon").checked;
    const body = { source: $("#in-src").value, gain_db: parseFloat($("#in-gain").value), record: $("#in-rec").checked, monitor: mon };
    if (mon && !inp.monitor) {
      if (!(await ask("Send the input to the DAC?", "A microphone near the speakers can feed back. Keep the master level low the first time.", "Monitor to DAC"))) return;
      body.confirm = true;
    }
    try { await post("/input", body); toast("Input saved"); renderDevices(); } catch (e) { fail(e); }
  };
  $$("[data-snap]").forEach((b) => b.onclick = async () => {
    try { b.disabled = true; await post(`/devices/${encodeURIComponent(b.dataset.snap)}/snapshot`); toast("Profile stored from the device"); renderDevices(); } catch (e) { fail(e); b.disabled = false; }
  });
  $$("[data-reapply]").forEach((b) => b.onclick = async () => {
    const n = b.dataset.n;
    if (!(await ask("Send stored settings?", `Send ${esc(n)} stored setting(s) to this Trillium Zero. Settings live in its RAM until it restarts.`, "Send settings"))) return;
    try { b.disabled = true; const r = await post(`/devices/${encodeURIComponent(b.dataset.reapply)}/reapply`, { confirm: true }); toast(`Re-applied ${r.changed.length} setting(s)`); renderDevices(); } catch (e) { fail(e); b.disabled = false; }
  });
  $$("[data-tele]").forEach((b) => b.onclick = async () => {
    try {
      b.disabled = true;
      const r = await get(`/devices/${encodeURIComponent(b.dataset.tele)}/telemetry?hz=20&ms=1000`);
      const f = r.frames[r.frames.length - 1];
      $(`#tele-${CSS.escape(b.dataset.tele)}`).textContent = f ? `${r.frames.length} frames · pitch ${f.pitch.toFixed(1)}° · roll ${f.roll.toFixed(1)}° · accel ${f.accel.join(", ")} mg · touch ${f.touching ? (f.touch_pos * 100).toFixed(0) + "%" : "off"} · taps ${f.taps}` : "no frames";
    } catch (e) { fail(e); } finally { b.disabled = false; }
  });
  $$("[data-as-input]").forEach((b) => b.onclick = async () => {
    try { await post("/input", { source: b.dataset.asInput }); toast("Input source saved"); renderDevices(); } catch (e) { fail(e); }
  });
  $$("[data-mic]").forEach((b) => b.onchange = async () => {
    const on = b.checked;
    if (on && !(await ask("Start the microphone test stream?", "This writes ACTION 7 to the device. The stream appears as <span class=\"mono\">Trillium Zero Mic</span> and is not sent to the DAC unless you turn monitoring on.", "Start stream"))) {
      b.checked = false;
      return;
    }
    b.disabled = true;
    try {
      await post(`/devices/${encodeURIComponent(b.dataset.mic)}/mic`, { on, confirm: on });
      toast(on ? "Microphone test stream on" : "Microphone test stream off");
      renderDevices();
    } catch (e) { fail(e); b.checked = !on; b.disabled = false; }
  });
  (d.zeros || []).forEach((z) => drawZeroPlot($("#plot-" + z.key), z.frames || []));
  if (!S.devTimer) S.devTimer = setInterval(tickDevices, 400);
}

function inputGlobFor(u) {
  const p = u.product || "";
  if (/yeti/i.test(p)) return "Yeti Nano*:capture_*";
  if (u.id === "1915:1025" || /composite/i.test(p)) return "USB Composite Device*:capture_*";
  return p ? `${p}*:capture_*` : "";
}

function tickDevices() {
  if (S.view !== "devices") { clearInterval(S.devTimer); S.devTimer = null; return; }
  get("/devices").then((d) => {
    for (const z of d.zeros || []) {
      const c = $("#plot-" + z.key);
      if (c) drawZeroPlot(c, z.frames || []);
      const live = $("#tele-live-" + z.key);
      const f = (z.frames || [])[(z.frames || []).length - 1];
      const strip = (z.sensors || []).some((s) => s.id === "trill" && s.state === "running");
      if (live && f) live.textContent = `pitch ${f.pitch.toFixed(1)}° · roll ${f.roll.toFixed(1)}°${strip ? " · " + (f.touching ? "touch " + (f.touch_pos * 100).toFixed(0) + "%" : "no touch") : ""} · taps ${f.taps}`;
      const sh = $("#sensors-" + z.key);
      if (sh) { const h = sensorsHtml(z); if (sh.dataset.h !== h) { sh.innerHTML = h; sh.dataset.h = h; } }
      const nh = $("#notepath-" + z.key);
      if (nh) { const h = notePathHtml(z); if (nh.dataset.h !== h) { nh.innerHTML = h; nh.dataset.h = h; } }
    }
  }).catch(() => {});
}

function drawZeroPlot(canvas, frames) {
  if (!canvas || !canvas.getContext) return;
  const ctx = canvas.getContext("2d");
  const w = canvas.width = Math.max(200, canvas.clientWidth || 600);
  const h = canvas.height = 140;
  const cs = getComputedStyle(document.body);
  const ink = cs.getPropertyValue("--text") || "#ddd";
  const dim = cs.getPropertyValue("--muted") || "#888";
  const acc = cs.getPropertyValue("--accent") || "#6cf";
  const met = cs.getPropertyValue("--meter") || "#e8a54b";
  ctx.clearRect(0, 0, w, h);
  ctx.strokeStyle = dim;
  ctx.globalAlpha = 0.35;
  ctx.beginPath();
  ctx.moveTo(0, h / 2); ctx.lineTo(w, h / 2);
  ctx.stroke();
  ctx.globalAlpha = 1;
  if (!frames.length) {
    ctx.fillStyle = dim;
    ctx.font = "12px ui-monospace, monospace";
    ctx.fillText("Waiting for sensor frames…", 8, 20);
    return;
  }
  const n = frames.length;
  const line = (key, color, scale) => {
    ctx.beginPath();
    ctx.strokeStyle = color;
    frames.forEach((f, i) => {
      const x = n === 1 ? w / 2 : i / (n - 1) * (w - 2) + 1;
      const y = h / 2 - (scale(f) / 90) * (h / 2 - 4);
      if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    });
    ctx.stroke();
  };
  line("pitch", acc.trim() || "#6cf", (f) => f.pitch);
  line("roll", met.trim() || "#e8a54b", (f) => f.roll);
  ctx.fillStyle = dim;
  ctx.font = "11px ui-monospace, monospace";
  ctx.fillText("pitch", 8, 14);
  ctx.fillStyle = (met.trim() || "#e8a54b");
  ctx.fillText("roll", 52, 14);
}

const SENSOR_BADGE = {
  running: ["ok", "running"], on: ["ok", "on"], present: ["ok", "present"],
  off: ["off", "off"], not_fitted: ["off", "not fitted"], absent: ["off", "absent"], unavailable: ["off", "unavailable"], checking: ["off", "checking"],
  lost: ["caution", "lost"], disabled: ["caution", "disabled"],
};

// Sensor inventory table: state is always written as text, never colour alone.
function sensorsHtml(z) {
  const rows = (z.sensors || []).map((s) => {
    const b = SENSOR_BADGE[s.state] || ["off", s.state];
    return `<tr><th scope="row" style="font-weight:500;text-align:left">${esc(s.label)}</th><td><span class="badge ${b[0]}">${esc(b[1])}</span></td><td class="muted">${esc(s.detail)}</td></tr>`;
  }).join("") || `<tr><td class="empty" colspan="3">Waiting for the device.</td></tr>`;
  return `<table class="sensors"><caption class="sr">Sensors on this Trillium Zero</caption><thead><tr><th scope="col" style="text-align:left">Sensor</th><th scope="col" style="text-align:left">State</th><th scope="col" style="text-align:left">Detail</th></tr></thead><tbody>${rows}</tbody></table>`;
}

function notePathHtml(z) {
  const np = z.note_path || {};
  if (!np.summary) return "";
  return `<div class="muted" style="margin-top:8px">Notes come from: <span class="mono">${esc(np.summary)}</span> <span class="badge ${np.strike_ready ? "ok" : "caution"}">${np.strike_ready ? "strike ready" : "strikes play nothing"}</span></div>${np.hint ? `<div class="risk caution" style="margin-top:6px"><b>!</b> ${esc(np.hint)}</div>` : ""}`;
}

function zeroCard(z) {
  const h = z.hello || {}, st = z.status || {};
  const batt = st.battery === "present" ? `${st.charge_pct ?? "?"}% · ${st.battery_mv} mV${st.charging ? " · charging" : ""}` : (st.battery === "absent" ? "absent (switch off or unplugged)" : (st.battery || "—"));
  const rows = (z.params || []).map((p) => {
    const diff = p.stored != null && p.stored !== p.value;
    return `<tr><td class="mono muted">${p.id}</td><td>${esc(p.name)}</td><td class="mono">${p.value}</td><td class="mono ${diff ? "" : "muted"}">${p.stored ?? "—"}</td><td>${diff ? '<span class="badge caution">differs</span>' : ""}</td></tr>`;
  }).join("");
  const nd = (z.differs || []).length;
  const state = { stored: ["ok", "profile stored"], matches: ["ok", "matches profile"], differs: ["caution", "differs from profile"], none: ["off", "no profile"] }[z.profile_state] || ["off", z.profile_state || "—"];
  return `<div class="card">
    <div class="card-head"><h3>Trillium Zero</h3><span class="muted mono" style="margin-left:8px">${esc(z.key)}</span><span class="spacer"></span>
      ${z.found ? "" : '<span class="badge off">disconnected</span> '}<span class="badge ${state[0]}">${esc(state[1])}</span></div>
    ${z.error ? `<div class="risk caution"><b>!</b> ${esc(z.error)}</div>` : ""}
    <div class="row">
      <div class="field"><label>Firmware</label><div class="mono">${esc(h.firmware || "—")}</div></div>
      <div class="field"><label>Battery</label><div class="mono">${esc(batt)}</div></div>
      <div class="field"><label>Link</label><div class="mono">${esc(st.link || "—")}</div></div>
      <div class="field"><label>Read</label><div class="mono">${esc(z.read_at_text || "—")}</div></div>
      <div class="field"><label>Profile</label><div class="mono">${z.profile ? esc(z.profile.captured_text) + " · " + z.profile.count + " settings" : "—"}</div></div>
    </div>
    ${z.reconnected && nd ? `<div class="risk caution" style="margin-top:8px"><b>!</b> Reconnected with ${nd} setting(s) different from its stored profile. Nothing is sent until you re-apply.</div>` : ""}
    <section aria-labelledby="sens-h-${esc(z.key)}" style="margin-top:12px">
      <div class="lbl" id="sens-h-${esc(z.key)}" style="margin-bottom:4px">Sensors</div>
      <div id="sensors-${esc(z.key)}" aria-live="polite">${sensorsHtml(z)}</div>
      <div id="notepath-${esc(z.key)}" aria-live="polite">${notePathHtml(z)}</div>
    </section>
    <canvas id="plot-${esc(z.key)}" width="600" height="140" style="width:100%;height:140px;margin-top:10px;border-radius:6px;background:rgba(0,0,0,0.2)"></canvas>
    <div class="muted" style="margin-top:4px">Live sensors at 20 Hz · <span class="mono" id="tele-live-${esc(z.key)}"></span>. Pitch and roll also leave the patch panel as <span class="mono">Trillium Synth:zero_motion</span> (MIDI CC 16 / 17 / 18), not connected automatically.</div>
    <div class="row" style="margin-top:10px;align-items:center">
      <div class="field"><label>Microphone test stream</label><label class="switch"><input type="checkbox" data-mic="${esc(z.key)}" ${z.mic ? "checked" : ""} ${z.found ? "" : "disabled"}><span></span></label></div>
      <div class="muted" style="flex:1">${z.mic ? "On: 16 kHz PDM capture, resampled to 48 kHz as <span class=\"mono\">Trillium Zero Mic</span> (selectable as the input strip; DAC monitor stays off unless confirmed). Stored in this device's profile and restored on reconnect." : "Off at firmware boot. Turning it on writes ACTION 7 to the device and opens the PCM port. Not part of the instrument mapping."}</div>
    </div>
    <div style="margin:10px 0;display:flex;gap:8px;align-items:center">
      <button class="btn small ghost" data-snap="${esc(z.key)}" ${z.found ? "" : "disabled"}>Store current settings as profile</button>
      ${nd && z.found ? `<button class="btn small" data-reapply="${esc(z.key)}" data-n="${nd}">Re-apply ${nd} stored setting(s)…</button>` : ""}
      <button class="btn small ghost" data-tele="${esc(z.key)}" ${z.found ? "" : "disabled"}>Sample sensors</button>
      <span class="muted mono" id="tele-${esc(z.key)}"></span>
    </div>
    <table><thead><tr><th>#</th><th>Setting</th><th>Device</th><th>Stored</th><th></th></tr></thead><tbody>${rows}</tbody></table>
  </div>`;
}

async function renderSettings() {
  let r, recs, ver = {}, eps = [], aud = { sinks: [] };
  try { r = await get("/settings"); recs = await get("/recordings"); } catch (e) { fail(e); return; }
  // About, API reference and the DAC list are extras: the page works without them.
  [ver, eps, aud] = await Promise.all([get("/versions").catch(() => ({})), get("/endpoints").catch(() => []), get("/audio/devices").catch(() => ({ sinks: [] }))]);
  if (S.view !== "settings") return;
  const st = r.settings; S.settings = st;
  const sys = S.sys || {};
  const recRows = recs.recordings.slice(0, 12).map((c) => `<tr><td class="mono small">${esc(c.id)}</td><td class="muted small">${esc(c.created.replace("T", " ").slice(0, 19))}</td><td class="num">${c.duration_s.toFixed(1)} s</td><td>${c.filed ? `<span class="badge ok">filed</span>` : `<span class="badge caution">only on this device</span>`}</td></tr>`).join("") || `<tr><td class="empty" colspan="4">No recordings yet. Press Rec in the header to make one.</td></tr>`;
  const locs = ["~/.local/share/pilab/recordings", "/srv/library/pilab-library/recordings"];
  if (!locs.includes(st.recording.location)) locs.push(st.recording.location);
  const urls = [`http://${(sys.hostname || "pilab")}.local:${sys.port || 8410}/`].concat((sys.addresses || []).filter((a) => !a.includes(":")).map((a) => `http://${a}:${sys.port || 8410}/`));
  $("#main").innerHTML = head("Settings", "Recording, archival, audio, MIDI and network. Each switch shows what it changes.", `<button class="btn primary" id="set-save">Save settings</button>`) + `
  <div class="grid g2">
    <div class="card">
      <div class="card-head"><h3>Local recording</h3><span class="spacer"></span><label class="switch"><input type="checkbox" id="rec-en" ${st.recording.enabled ? "checked" : ""}><span></span></label></div>
      <div class="risk">Records the synth output (post limiter) as 32-bit float WAV with a record card, and the audio input strip when recording of the input is on. Local may be enough; recordings are masters until filed to the NAS Archive.</div>
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
        <div class="field" style="flex:1"><label>Target share</label><input class="input mono" id="arc-target" value="${esc(st.archive.target)}" placeholder="//nas-host/Recordings"></div>
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
      <div class="field" style="margin-top:8px"><label>DAC sink (watchdog)</label><input class="input mono" id="aud-sink" list="aud-sinks" value="${esc(st.audio.dac_sink)}"><datalist id="aud-sinks">${(aud.sinks || []).filter((k) => !k.airplay).map((k) => `<option value="${esc(k.name)}">${esc(k.description || k.kind || "")}</option>`).join("")}</datalist></div>
      <div class="row" style="margin-top:10px"><label class="switch"><input type="checkbox" id="aud-start" ${st.audio.start_engine ? "checked" : ""}><span></span></label><span>Start the synth when the service starts (fade-in, −18 dBFS max)</span></div>
      <div class="lbl" style="margin:14px 0 6px">Dropout watchdog</div>
      <div class="row"><label class="switch"><input type="checkbox" id="wd-en" ${st.watchdog.enabled ? "checked" : ""}><span></span></label><span>Suspend and resume the DAC sink after</span>
        <input class="input mono" id="wd-rate" aria-label="Dropouts per second" type="number" min="1" step="1" value="${st.watchdog.xruns_per_s}" style="width:70px"><span>dropouts/s for</span>
        <input class="input mono" id="wd-hold" aria-label="Seconds over the limit" type="number" min="2" step="1" value="${st.watchdog.hold_s}" style="width:70px"><span>s</span></div>
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
      <div class="muted small">Edit <code>[[midi.devices]]</code> in <code>~/.config/pilab/settings.toml</code> on the device, then restart <code>pilabd</code>.</div>
      <div class="lbl" style="margin:14px 0 6px">Network and pairing</div>
      <div class="kv">
        <span class="k">Console</span><span class="v">${urls.map(esc).join("<br>")}</span>
        <span class="k">mDNS</span><span class="v">_trillium-base._tcp · _pilab._tcp · _http._tcp</span>
        <span class="k">Device id</span><span class="v">${esc(sys.device_id || "")}</span>
      </div>
      <div class="risk caution" style="margin-top:10px"><b>!</b> The console is plain HTTP on the LAN. Anyone on this network with the pairing token can change the sound. On venue networks, prefer a direct cable.</div>
      <div class="row" style="margin-top:10px"><button class="btn ghost small" id="forget">Forget token in this browser</button><span class="muted small">New token on the device: <code>trillium-base pair --rotate</code></span></div>
    </div>
    <div class="card">
      <div class="card-head"><h3>About</h3></div>
      <div class="about-brand"><img src="/brand/lockup-primary.svg" alt="Trillium"><span class="muted">Base · Console</span></div>
      <div class="kv">
        <span class="k">Console</span><span class="v">${esc((ver.console && ver.console.version) || (S.info && S.info.version) || "—")} · <span class="mono">${esc((ver.console && ver.console.commit) || (S.info && S.info.commit) || "")}</span></span>
        <span class="k">Device</span><span class="v">${esc(sys.hostname || (S.info && S.info.hostname) || "—")} · ${esc(sys.device_id || "")}</span>
        <span class="k">Hardware</span><span class="v small">${esc(ver.model || "—")}</span>
        <span class="k">OS · kernel</span><span class="v small">${esc(ver.os || "—")} · ${esc(ver.kernel || "—")}</span>
        <span class="k">Audio</span><span class="v small">PipeWire ${esc(ver.pipewire || "—")}</span>
        <span class="k">Licence</span><span class="v small">GPL-3.0-or-later OR commercial</span>
      </div>
      <div class="about-links">
        <a href="https://m-of-n.com/ambient-lab/trillium/base/" target="_blank" rel="noopener noreferrer">Trillium Base</a>
        <a href="https://github.com/nymble/trillium-base" target="_blank" rel="noopener noreferrer">Source</a>
        <a href="https://github.com/nymble/trillium-base/releases" target="_blank" rel="noopener noreferrer">Releases</a>
        <a href="mailto:support@m-of-n.com">Support</a>
      </div>
      <div class="muted small" style="margin-top:12px">Copyright (C) 2026 M-of-N. Trillium, Trillium Base, Trillium Console, Trillium Synth, Ambient Lab and the Rhodonea mark are trademarks of M-of-N. The licences cover the code, not the names and marks.</div>
    </div>
    <div class="card">
      <div class="card-head"><h3>API reference</h3><span class="spacer"></span><span class="muted small mono">GET /api/v1/endpoints</span></div>
      <div class="muted small" style="margin-bottom:8px">Everything this console does goes through this API. Send the pairing token as a Bearer token.</div>
      <div class="api-list">${eps.length ? eps.map((e) => `<div><span class="m">${esc(e.method)}</span><span class="p">${esc(e.path.replace("/api/v1", ""))}</span><span class="d">${esc(e.what)}</span></div>`).join("") : `<div class="empty-state">The endpoint list is not available.</div>`}</div>
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

$("#btn-kbd").addEventListener("click", () => { const h = $("#kbd").classList.toggle("hidden"); $("#btn-kbd").setAttribute("aria-expanded", h ? "false" : "true"); $("#btn-kbd").classList.toggle("on", !h); });
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
