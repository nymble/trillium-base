// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
// Trillium Console: Music page. Library (albums, artists, songs, playlists, recordings,
// radio), search, album detail, now-playing bar and queue. Playback is on the unit (MPD
// through the DAC); this page is the remote. Loaded before app.js and uses its helpers
// ($, $$, esc, get, post, put, api, toast, fail, head, dialog, askText, ask, playRecording).
"use strict";

const MU = {
  tab: localStorage.getItem("pilab.music.tab") || "albums",
  detail: null,        // { type: "album"|"artist"|"playlist", id }
  q: "",
  st: null,            // last /music/status
  stAt: 0,             // when it arrived (for the moving seek bar)
  art: new Map(),      // art key -> object URL
  artWait: new Map(),
  queueOpen: false,
  sourcesOpen: false,
  poll: 0,
  seeking: false,
  volBusy: 0,
  data: {},
};
const MU_ICON = {
  prev: '<path d="M6 5v14"/><path d="M19 5 9 12l10 7z" fill="currentColor"/>',
  next: '<path d="M18 5v14"/><path d="m5 5 10 7-10 7z" fill="currentColor"/>',
  play: '<path d="m7 4 13 8-13 8z" fill="currentColor"/>',
  pause: '<rect x="6" y="4.5" width="4" height="15" rx="1" fill="currentColor"/><rect x="14" y="4.5" width="4" height="15" rx="1" fill="currentColor"/>',
  shuffle: '<path d="M3 7h3.5c2.5 0 3.9 1.3 5.5 5s3 5 5.5 5H21"/><path d="M3 17h3.5c1.4 0 2.4-.4 3.3-1.3"/><path d="M14.2 8.3C15 7.4 16 7 17.5 7H21"/><path d="m18 4 3 3-3 3"/><path d="m18 14 3 3-3 3"/>',
  repeat: '<path d="M17 2.5 20.5 6 17 9.5"/><path d="M3.5 11.5V10a4 4 0 0 1 4-4h13"/><path d="M7 21.5 3.5 18 7 14.5"/><path d="M20.5 12.5V14a4 4 0 0 1-4 4h-13"/>',
  queue: '<path d="M4 6h12M4 12h12M4 18h8"/><path d="m16 15 5 3-5 3z" fill="currentColor"/>',
  vol: '<path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z" fill="currentColor"/><path d="M15.5 9a4 4 0 0 1 0 6"/><path d="M18 6.5a7.5 7.5 0 0 1 0 11"/>',
};
const muIcon = (n) => `<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${MU_ICON[n]}</svg>`;
const MU_TABS = [["albums", "Albums"], ["artists", "Artists"], ["songs", "Songs"], ["playlists", "Playlists"], ["recordings", "Recordings"], ["radio", "Radio"]];

const muTime = (s) => {
  s = Math.max(0, Math.floor(s || 0));
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), x = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${x}` : `${m}:${x}`;
};
const muTotal = (s) => { const m = Math.round((s || 0) / 60); return m >= 60 ? `${Math.floor(m / 60)} hr ${m % 60} min` : `${m} min`; };
const muInitials = (t) => esc(String(t || "?").replace(/^the /i, "").split(/\s+/).slice(0, 2).map((w) => w[0] || "").join("").toUpperCase() || "♪");

// ------------------------------------------------------------------ cover art (fetched with the token)
function muArt(key, title, cls = "") {
  return `<div class="mu-art ${cls}" ${key ? `data-art="${esc(key)}"` : ""} aria-hidden="true"><span>${muInitials(title)}</span></div>`;
}
let muArtObs = null;
function muHydrateArt(root) {
  if (!muArtObs) {
    muArtObs = new IntersectionObserver((ents) => ents.forEach((e) => { if (e.isIntersecting) { muArtObs.unobserve(e.target); muLoadArt(e.target); } }), { rootMargin: "300px" });
  }
  $$(".mu-art[data-art]", root).forEach((el) => {
    const k = el.dataset.art;
    if (MU.art.has(k)) muSetArt(el, MU.art.get(k)); else muArtObs.observe(el);
  });
}
function muSetArt(el, url) {
  if (!url || el.querySelector("img")) return;
  const img = new Image(); img.alt = ""; img.decoding = "async"; img.src = url;
  img.onload = () => el.classList.add("has-img");
  el.appendChild(img);
}
async function muLoadArt(el) {
  const k = el.dataset.art, size = el.classList.contains("lg") ? 600 : 300;
  const id = `${k}-${size}`;
  if (!MU.artWait.has(id)) {
    MU.artWait.set(id, fetch(`/api/v1/music/art/${encodeURIComponent(k)}?size=${size}`, { headers: { Authorization: `Bearer ${S.token}` } })
      .then((r) => (r.ok ? r.blob() : null)).then((b) => (b ? URL.createObjectURL(b) : null)).catch(() => null));
  }
  const url = await MU.artWait.get(id);
  if (url && size === 300) MU.art.set(k, url);
  muSetArt(el, url);
}

// ------------------------------------------------------------------ player actions
async function muPlay(body, msg) {
  try {
    const r = await post("/music/play", body);
    if (msg !== false) toast(msg || (r.mode === "replace" ? "Playing on this unit" : r.mode === "next" ? `Playing next (${r.queued})` : `Added to the queue (${r.queued})`));
    muPollNow();
    if (MU.queueOpen) muQueue();
  } catch (e) { fail(e); }
}
async function muCtl(action, value) {
  try { const r = await post("/music/control", { action, value }); if (r.capped) toast(`Volume is capped at ${r.volume_max}% on this unit`); MU.st && (MU.st.player = r.player, MU.stAt = performance.now()); muBar(); } catch (e) { fail(e); }
}
async function muAddToPlaylist(items) {
  let pls = [];
  try { pls = (await get("/music/playlists")).playlists; } catch (e) { return fail(e); }
  const opts = pls.map((p) => `<button class="btn ghost mu-pick" data-pl="${p.id}">${esc(p.name)} <span class="muted small">${p.count}</span></button>`).join("");
  const d = document.createElement("dialog");
  d.className = "dlg";
  d.innerHTML = `<h3>Add to playlist</h3><div class="dlg-body mu-picklist">${opts || `<p class="muted">No playlists yet.</p>`}</div>
    <div class="row" style="justify-content:space-between;margin-top:14px"><button class="btn ghost" data-x="new">New playlist…</button><button class="btn ghost" data-x="cancel">Cancel</button></div>`;
  document.body.appendChild(d);
  const close = () => { d.close(); d.remove(); };
  const add = async (id, name) => {
    close();
    try { const r = await post(`/music/playlists/${id}/items`, { items }); toast(`Added ${r.added} to ${name}`); if (MU.tab === "playlists") muBody(); } catch (e) { fail(e); }
  };
  $$("[data-pl]", d).forEach((b) => b.onclick = () => add(b.dataset.pl, b.textContent.trim()));
  $('[data-x="cancel"]', d).onclick = close;
  $('[data-x="new"]', d).onclick = async () => {
    close();
    const name = await askText("New playlist", "Name", "", "Create");
    if (!name) return;
    try { const r = await post("/music/playlists", { name }); await add(r.id, name); } catch (e) { fail(e); }
  };
  d.addEventListener("cancel", close);
  d.showModal();
}
// Small action sheet for a song or take: play next, add to queue, add to playlist.
function muMenu(ev, kind, id, title) {
  ev.stopPropagation();
  const d = document.createElement("dialog");
  d.className = "dlg mu-sheet";
  const canList = kind === "track" || kind === "take" || kind === "album";
  d.innerHTML = `<h3>${esc(title)}</h3><div class="mu-picklist">
    <button class="btn ghost" data-a="next">Play next</button>
    <button class="btn ghost" data-a="append">Add to queue</button>
    ${canList ? `<button class="btn ghost" data-a="pl">Add to playlist…</button>` : ""}
    <button class="btn ghost" data-a="cancel">Cancel</button></div>`;
  document.body.appendChild(d);
  const close = () => { d.close(); d.remove(); };
  $$("[data-a]", d).forEach((b) => b.onclick = () => {
    close();
    const a = b.dataset.a;
    if (a === "next" || a === "append") muPlay({ kind, id, mode: a });
    if (a === "pl") muAddToPlaylist([{ kind, id }]);
  });
  d.addEventListener("cancel", close);
  d.showModal();
}

// ------------------------------------------------------------------ page
async function renderMusic() {
  $("#main").innerHTML = `<div class="mu">
    ${head("Music", `<span id="mu-sub">Your library, played on this unit through the DAC</span>`, `<div class="mu-head-r">
      <label class="mu-search"><span class="sr">Search music</span><input class="input" id="mu-q" type="search" placeholder="Search albums, artists, songs" autocomplete="off" value="${esc(MU.q)}"></label>
      <button class="btn ghost" id="mu-src-btn" aria-expanded="${MU.sourcesOpen}">Library</button></div>`)}
    <div class="mu-tabs" role="tablist" aria-label="Music library">${MU_TABS.map(([k, l]) => `<button role="tab" data-tab="${k}" aria-selected="${!MU.q && !MU.detail && MU.tab === k}" class="${!MU.q && MU.tab === k ? "on" : ""}">${l}</button>`).join("")}</div>
    <div id="mu-sources" class="${MU.sourcesOpen ? "" : "hidden"}"></div>
    <div id="mu-body" class="mu-body"></div>
    <aside id="mu-queue" class="mu-queue ${MU.queueOpen ? "" : "hidden"}" aria-label="Play queue"></aside>
    <div id="mu-bar" class="mu-bar" role="region" aria-label="Now playing"></div>
  </div>`;
  $$(".mu-tabs [data-tab]").forEach((b) => b.onclick = () => { MU.tab = b.dataset.tab; MU.detail = null; MU.q = ""; $("#mu-q").value = ""; localStorage.setItem("pilab.music.tab", MU.tab); muTabsSync(); muBody(); });
  $("#mu-q").addEventListener("input", (e) => debounce("mu-q", () => { MU.q = e.target.value.trim(); MU.detail = null; muTabsSync(); muBody(); }, 220));
  $("#mu-src-btn").onclick = () => { MU.sourcesOpen = !MU.sourcesOpen; $("#mu-src-btn").setAttribute("aria-expanded", MU.sourcesOpen); $("#mu-sources").classList.toggle("hidden", !MU.sourcesOpen); if (MU.sourcesOpen) muSources(); };
  if (MU.sourcesOpen) muSources();
  muBar();
  muBody();
  if (MU.queueOpen) muQueue();
  muStartPoll();
}
function muTabsSync() {
  $$(".mu-tabs [data-tab]").forEach((b) => { const on = !MU.q && MU.tab === b.dataset.tab; b.classList.toggle("on", on); b.setAttribute("aria-selected", on && !MU.detail); });
}
async function muBody() {
  const el = $("#mu-body");
  if (!el) return;
  try {
    if (MU.q) await muSearch(el);
    else if (MU.detail && MU.detail.type === "album") await muAlbum(el, MU.detail.id);
    else if (MU.detail && MU.detail.type === "artist") await muArtist(el, MU.detail.id);
    else if (MU.detail && MU.detail.type === "playlist") await muPlaylist(el, MU.detail.id);
    else await ({ albums: muAlbums, artists: muArtists, songs: muSongs, playlists: muPlaylists, recordings: muTakes, radio: muRadio }[MU.tab] || muAlbums)(el);
  } catch (e) { el.innerHTML = `<div class="empty-state">${esc(e.message || e)}</div>`; }
  muHydrateArt(el);
  muMarkPlaying();
}
const muOpen = (type, id) => { MU.detail = { type, id }; muTabsSync(); muBody(); $("#main").scrollTop = 0; };
const muBack = (label) => `<button class="btn ghost small mu-back" data-back>‹ ${esc(label)}</button>`;
function muBindBack(el) { $$("[data-back]", el).forEach((b) => b.onclick = () => { MU.detail = null; muTabsSync(); muBody(); }); }

function muAlbumTile(a) {
  return `<div class="mu-tile" data-album="${a.id}" tabindex="0" role="button" aria-label="${esc(a.title)} by ${esc(a.artist)}">
    <div class="mu-cover">${muArt(a.art, a.title)}<button class="mu-tile-play" data-play-album="${a.id}" aria-label="Play ${esc(a.title)}" title="Play">▶</button></div>
    <div class="mu-t1">${esc(a.title)}</div><div class="mu-t2">${esc(a.artist)}${a.year ? ` · ${a.year}` : ""}</div></div>`;
}
function muBindTiles(el) {
  $$("[data-album]", el).forEach((t) => {
    t.onclick = () => muOpen("album", t.dataset.album);
    t.onkeydown = (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); t.click(); } };
  });
  $$("[data-play-album]", el).forEach((b) => b.onclick = (e) => { e.stopPropagation(); muPlay({ kind: "album", id: b.dataset.playAlbum }); });
}

async function muAlbums(el) {
  const sort = localStorage.getItem("pilab.music.sort") || "artist";
  const r = await get(`/music/albums?sort=${sort}`);
  if (MU.tab !== "albums" || MU.detail || MU.q) return;
  const st = MU.st && MU.st.scan;
  el.innerHTML = `<div class="mu-bar-top"><div class="seg" id="mu-sort">${[["artist", "Artist"], ["title", "Title"], ["year", "Year"], ["added", "Recently added"]].map(([k, l]) => `<button data-s="${k}" class="${sort === k ? "on" : ""}">${l}</button>`).join("")}</div><span class="muted small">${r.albums.length} albums</span></div>
    ${r.albums.length ? `<div class="mu-grid">${r.albums.map(muAlbumTile).join("")}</div>` : `<div class="empty-state">${st && st.running ? "Scanning the library…" : "No albums yet. Add a folder or plug in the library drive, then scan (Library)."}</div>`}`;
  $$("#mu-sort [data-s]", el).forEach((b) => b.onclick = () => { localStorage.setItem("pilab.music.sort", b.dataset.s); muBody(); });
  muBindTiles(el);
}

function muTrackRows(tracks, opts = {}) {
  return tracks.map((t, i) => `<div class="mu-row" data-i="${i}" data-track="${t.id}" tabindex="0" role="button" aria-label="Play ${esc(t.title)}">
      ${opts.art ? muArt(t.art, t.album, "sm") : `<span class="mu-no">${t.no || i + 1}</span>`}
      <div class="grow"><div class="mu-t1">${esc(t.title)}</div>${opts.artist || opts.art || (opts.albumArtist && t.artist !== opts.albumArtist) ? `<div class="mu-t2">${esc(t.artist)}${opts.art ? ` · ${esc(t.album)}` : ""}</div>` : ""}</div>
      <span class="mu-dur mono">${muTime(t.duration)}</span>
      <button class="btn ghost mu-more" data-more="${t.id}" aria-label="More for ${esc(t.title)}" title="Play next, add to queue or playlist">⋯</button>
    </div>`).join("");
}
function muBindTrackRows(el, tracks, playBody) {
  $$(".mu-row[data-track]", el).forEach((row) => {
    row.onclick = () => muPlay(playBody(+row.dataset.i), false);
    row.onkeydown = (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); row.click(); } };
  });
  $$("[data-more]", el).forEach((b) => { const t = tracks.find((x) => String(x.id) === b.dataset.more); b.onclick = (e) => muMenu(e, "track", t.id, t.title); });
}

async function muAlbum(el, id) {
  const r = await get(`/music/albums/${id}`);
  const a = r.album;
  el.innerHTML = `${muBack(MU.tab === "artists" ? "Artists" : MU.q ? "Search" : "Albums")}
    <div class="mu-hero">${muArt(a.art, a.title, "lg")}
      <div class="mu-hero-txt"><h2>${esc(a.title)}</h2>
        <button class="mu-link" data-artist="${a.artist_id}">${esc(a.artist)}</button>
        <div class="muted small">${[a.genre, a.year || "", `${a.tracks} songs, ${muTotal(a.duration)}`].filter(Boolean).map(esc).join(" · ")}</div>
        ${a.format ? `<div><span class="badge" title="As stored; PipeWire plays at 48 kHz">${esc(a.format)}</span>${a.mbid ? ` <span class="badge" title="MusicBrainz release ${esc(a.mbid)}">MusicBrainz</span>` : ""}</div>` : ""}
        <div class="mu-actions"><button class="btn primary" id="mu-a-play">▶ Play</button><button class="btn ghost" id="mu-a-shuf">Shuffle</button><button class="btn ghost" id="mu-a-next">Play next</button><button class="btn ghost" id="mu-a-q">Add to queue</button><button class="btn ghost" id="mu-a-pl">Add to playlist</button></div>
      </div></div>
    <div class="mu-list">${muTrackRows(r.tracks, { albumArtist: a.artist })}</div>`;
  muBindBack(el);
  $("[data-artist]", el).onclick = () => muOpen("artist", a.artist_id);
  $("#mu-a-play").onclick = () => muPlay({ kind: "album", id: a.id }, false);
  $("#mu-a-shuf").onclick = () => muPlay({ kind: "album", id: a.id, shuffle: true }, "Shuffling");
  $("#mu-a-next").onclick = () => muPlay({ kind: "album", id: a.id, mode: "next" });
  $("#mu-a-q").onclick = () => muPlay({ kind: "album", id: a.id, mode: "append" });
  $("#mu-a-pl").onclick = () => muAddToPlaylist([{ kind: "album", id: a.id }]);
  muBindTrackRows(el, r.tracks, (i) => ({ kind: "album", id: a.id, start: i }));
}

async function muArtists(el) {
  const r = await get("/music/artists");
  if (MU.tab !== "artists" || MU.detail || MU.q) return;
  el.innerHTML = r.artists.length ? `<div class="mu-list">${r.artists.map((a) => `<div class="mu-row" data-artist="${a.id}" tabindex="0" role="button">${muArt(a.art, a.name, "sm round")}<div class="grow"><div class="mu-t1">${esc(a.name)}</div><div class="mu-t2">${a.albums} album${a.albums === 1 ? "" : "s"} · ${a.tracks} songs</div></div><span class="muted">›</span></div>`).join("")}</div>` : `<div class="empty-state">No artists yet.</div>`;
  $$("[data-artist]", el).forEach((row) => { row.onclick = () => muOpen("artist", row.dataset.artist); row.onkeydown = (e) => { if (e.key === "Enter") row.click(); }; });
}

async function muArtist(el, id) {
  const r = await get(`/music/artists/${id}`);
  const a = r.artist;
  el.innerHTML = `${muBack("Artists")}
    <div class="mu-hero">${muArt(a.art, a.name, "lg round")}<div class="mu-hero-txt"><h2>${esc(a.name)}</h2><div class="muted small">${r.albums.length} albums · ${a.tracks} songs</div>
      <div class="mu-actions"><button class="btn primary" id="mu-ar-play">▶ Play</button><button class="btn ghost" id="mu-ar-shuf">Shuffle</button></div></div></div>
    <h3 class="mu-h">Albums</h3><div class="mu-grid">${r.albums.map(muAlbumTile).join("")}</div>
    ${r.tracks.length ? `<h3 class="mu-h">Appears on</h3><div class="mu-list">${muTrackRows(r.tracks, { art: true })}</div>` : ""}`;
  muBindBack(el);
  muBindTiles(el);
  $("#mu-ar-play").onclick = () => muPlay({ kind: "artist", id: a.id }, false);
  $("#mu-ar-shuf").onclick = () => muPlay({ kind: "artist", id: a.id, shuffle: true }, "Shuffling");
  muBindTrackRows(el, r.tracks, (i) => ({ kind: "tracks", ids: r.tracks.map((t) => t.id), start: i }));
}

async function muSongs(el) {
  const r = await get("/music/tracks?limit=5000");
  if (MU.tab !== "songs" || MU.detail || MU.q) return;
  el.innerHTML = `<div class="mu-bar-top"><button class="btn ghost" id="mu-s-shuf">Shuffle all</button><span class="muted small">${r.total} songs</span></div>
    ${r.tracks.length ? `<div class="mu-list">${muTrackRows(r.tracks, { art: true })}</div>` : `<div class="empty-state">No songs yet.</div>`}`;
  const ids = r.tracks.map((t) => t.id);
  const sh = $("#mu-s-shuf", el); if (sh) sh.onclick = () => muPlay({ kind: "tracks", ids, shuffle: true }, "Shuffling all songs");
  muBindTrackRows(el, r.tracks, (i) => ({ kind: "tracks", ids, start: i }));
}

async function muPlaylists(el) {
  const r = await get("/music/playlists");
  if (MU.tab !== "playlists" || MU.detail || MU.q) return;
  el.innerHTML = `<div class="mu-bar-top"><button class="btn primary" id="mu-pl-new">New playlist</button><span class="muted small">${r.playlists.length} playlists</span></div>
    ${r.playlists.length ? `<div class="mu-grid">${r.playlists.map((p) => `<div class="mu-tile" data-pl="${p.id}" tabindex="0" role="button"><div class="mu-cover">${muArt(p.art, p.name)}<button class="mu-tile-play" data-play-pl="${p.id}" aria-label="Play ${esc(p.name)}">▶</button></div><div class="mu-t1">${esc(p.name)}</div><div class="mu-t2">${p.count} items${p.duration ? ` · ${muTotal(p.duration)}` : ""}</div></div>`).join("")}</div>`
      : `<div class="empty-state">No playlists yet. Make one here, or use “Add to playlist” on an album or song.</div>`}`;
  $("#mu-pl-new", el).onclick = async () => {
    const name = await askText("New playlist", "Name", "", "Create");
    if (!name) return;
    try { const p = await post("/music/playlists", { name }); muOpen("playlist", p.id); } catch (e) { fail(e); }
  };
  $$("[data-pl]", el).forEach((t) => { t.onclick = () => muOpen("playlist", t.dataset.pl); t.onkeydown = (e) => { if (e.key === "Enter") t.click(); }; });
  $$("[data-play-pl]", el).forEach((b) => b.onclick = (e) => { e.stopPropagation(); muPlay({ kind: "playlist", id: b.dataset.playPl }); });
}

async function muPlaylist(el, id) {
  const r = await get(`/music/playlists/${id}`);
  const p = r.playlist;
  el.innerHTML = `${muBack("Playlists")}
    <div class="mu-hero">${muArt(p.art, p.name, "lg")}<div class="mu-hero-txt"><h2>${esc(p.name)}</h2><div class="muted small">${p.count} items${p.duration ? ` · ${muTotal(p.duration)}` : ""}</div>
      <div class="mu-actions"><button class="btn primary" id="mu-p-play" ${p.count ? "" : "disabled"}>▶ Play</button><button class="btn ghost" id="mu-p-shuf" ${p.count ? "" : "disabled"}>Shuffle</button><button class="btn ghost" id="mu-p-ren">Rename</button><button class="btn ghost" id="mu-p-del">Delete</button></div></div></div>
    <div class="mu-list">${r.items.map((it, i) => `<div class="mu-row ${it.missing ? "missing" : ""}" data-i="${i}" tabindex="0" role="button">
        ${it.kind === "track" ? muArt(it.art, it.subtitle, "sm") : `<div class="mu-art sm mu-glyph" aria-hidden="true"><span>${it.kind === "take" ? "●" : "≋"}</span></div>`}
        <div class="grow"><div class="mu-t1">${esc(it.title || it.url || "")}</div><div class="mu-t2">${it.missing ? "missing from the library" : esc(it.subtitle)}</div></div>
        <span class="mu-dur mono">${it.duration ? muTime(it.duration) : ""}</span>
        <button class="btn ghost mu-more" data-up="${i}" aria-label="Move up" ${i ? "" : "disabled"}>↑</button>
        <button class="btn ghost mu-more" data-rm="${it.pos}" aria-label="Remove ${esc(it.title)}">✕</button></div>`).join("") || `<div class="empty-state">Empty. Add albums, songs, recordings or radio with “Add to playlist”.</div>`}</div>`;
  muBindBack(el);
  $("#mu-p-play", el).onclick = () => muPlay({ kind: "playlist", id: p.id }, false);
  $("#mu-p-shuf", el).onclick = () => muPlay({ kind: "playlist", id: p.id, shuffle: true }, "Shuffling");
  $("#mu-p-ren", el).onclick = async () => { const n = await askText("Rename playlist", "Name", p.name, "Rename"); if (n) { try { await put(`/music/playlists/${p.id}`, { name: n }); muBody(); } catch (e) { fail(e); } } };
  $("#mu-p-del", el).onclick = async () => { if (await ask(`Delete “${esc(p.name)}”?`, "The songs stay in the library.", "Delete")) { try { await api("DELETE", `/music/playlists/${p.id}`); MU.detail = null; muBody(); } catch (e) { fail(e); } } };
  $$(".mu-row[data-i]", el).forEach((row) => row.onclick = (e) => { if (e.target.closest("button")) return; muPlay({ kind: "playlist", id: p.id, start: +row.dataset.i }, false); });
  $$("[data-rm]", el).forEach((b) => b.onclick = async (e) => { e.stopPropagation(); try { await api("DELETE", `/music/playlists/${p.id}/items/${b.dataset.rm}`); muBody(); } catch (er) { fail(er); } });
  $$("[data-up]", el).forEach((b) => b.onclick = async (e) => { e.stopPropagation(); const i = +b.dataset.up; try { await post(`/music/playlists/${p.id}/move`, { from: i, to: i - 1 }); muBody(); } catch (er) { fail(er); } });
}

async function muTakes(el) {
  const r = await get("/music/takes");
  if (MU.tab !== "recordings" || MU.detail || MU.q) return;
  const groups = {};
  r.takes.forEach((t) => (groups[t.session] = groups[t.session] || []).push(t));
  el.innerHTML = `<div class="mu-note muted small">Takes recorded on this unit (${esc(r.location)}). They stay separate from published music.</div><div id="mu-take-player"></div>
    ${Object.keys(groups).length ? Object.entries(groups).map(([s, ts]) => `<h3 class="mu-h">${esc(s)}</h3><div class="mu-list">${ts.map((t) => `<div class="mu-row" data-take="${esc(t.id)}" tabindex="0" role="button">
        <div class="mu-art sm mu-glyph" aria-hidden="true"><span>●</span></div>
        <div class="grow"><div class="mu-t1">${esc(t.title)}</div><div class="mu-t2">${esc([t.scene, t.source, t.sample_rate ? `${t.sample_rate / 1000} kHz` : "", t.filed ? "filed" : "on this unit"].filter(Boolean).join(" · "))}</div></div>
        <span class="mu-dur mono">${muTime(t.duration)}</span>
        <button class="btn ghost mu-more" data-browser="${esc(t.id)}" title="Play in this browser" aria-label="Play ${esc(t.title)} in this browser">⧉</button>
        <button class="btn ghost mu-more" data-tmore="${esc(t.id)}" aria-label="More for ${esc(t.title)}">⋯</button></div>`).join("")}</div>`).join("") : `<div class="empty-state">No recordings yet. Use Record to make a take.</div>`}`;
  $$("[data-take]", el).forEach((row) => row.onclick = (e) => { if (e.target.closest("button")) return; muPlay({ kind: "take", id: row.dataset.take }, false); });
  $$("[data-browser]", el).forEach((b) => b.onclick = (e) => { e.stopPropagation(); playRecording(b.dataset.browser, $("#mu-take-player")); });
  $$("[data-tmore]", el).forEach((b) => b.onclick = (e) => { const t = r.takes.find((x) => x.id === b.dataset.tmore); muMenu(e, "take", t.id, t.title); });
}

async function muRadio(el) {
  const r = await get("/music/streams");
  if (MU.tab !== "radio" || MU.detail || MU.q) return;
  el.innerHTML = `<div class="mu-list">${r.streams.map((s) => `<div class="mu-row" data-stream="${s.id}" tabindex="0" role="button">
      <div class="mu-art sm mu-glyph" aria-hidden="true"><span>≋</span></div>
      <div class="grow"><div class="mu-t1">${esc(s.name)}</div><div class="mu-t2 mono">${esc(s.url)}</div></div>
      <button class="btn ghost mu-more" data-pladd="${s.id}" aria-label="Add ${esc(s.name)} to a playlist" title="Add to playlist">＋</button>
      <button class="btn ghost mu-more" data-srm="${s.id}" aria-label="Remove ${esc(s.name)}">✕</button></div>`).join("") || `<div class="empty-state">No streams.</div>`}</div>
    <form class="mu-add" id="mu-add-stream"><h3 class="mu-h">Add a stream</h3>
      <input class="input" name="name" placeholder="Name" aria-label="Stream name" autocomplete="off">
      <input class="input mono" name="url" placeholder="https://…" aria-label="Stream URL" inputmode="url" autocomplete="off" required>
      <button class="btn primary" type="submit">Add</button></form>
    <div class="muted small mu-note">Internet radio plays on this unit only while you choose to. Streams are listed in ${esc(r.file)}.</div>`;
  $$("[data-stream]", el).forEach((row) => row.onclick = (e) => { if (e.target.closest("button")) return; muPlay({ kind: "stream", id: row.dataset.stream }, false); });
  $$("[data-srm]", el).forEach((b) => b.onclick = async (e) => { e.stopPropagation(); try { await api("DELETE", `/music/streams/${b.dataset.srm}`); muBody(); } catch (er) { fail(er); } });
  $$("[data-pladd]", el).forEach((b) => b.onclick = (e) => { e.stopPropagation(); const s = r.streams.find((x) => String(x.id) === b.dataset.pladd); muAddToPlaylist([{ kind: "url", id: s.url, title: s.name }]); });
  $("#mu-add-stream", el).onsubmit = async (e) => {
    e.preventDefault();
    const f = e.target;
    try { await post("/music/streams", { name: f.name.value, url: f.url.value }); toast("Stream added"); muBody(); } catch (er) { fail(er); }
  };
}

async function muSearch(el) {
  const q = MU.q;
  const r = await get(`/music/search?q=${encodeURIComponent(q)}&limit=24`);
  if (MU.q !== q) return;
  const n = r.albums.length + r.artists.length + r.tracks.length + r.takes.length + r.streams.length;
  el.innerHTML = !n ? `<div class="empty-state">Nothing matches “${esc(q)}”.</div>` : `
    ${r.artists.length ? `<h3 class="mu-h">Artists</h3><div class="mu-list">${r.artists.map((a) => `<div class="mu-row" data-artist="${a.id}" tabindex="0" role="button">${muArt(a.art, a.name, "sm round")}<div class="grow"><div class="mu-t1">${esc(a.name)}</div><div class="mu-t2">${a.albums} albums</div></div></div>`).join("")}</div>` : ""}
    ${r.albums.length ? `<h3 class="mu-h">Albums</h3><div class="mu-grid">${r.albums.map(muAlbumTile).join("")}</div>` : ""}
    ${r.tracks.length ? `<h3 class="mu-h">Songs</h3><div class="mu-list" id="mu-s-tracks">${muTrackRows(r.tracks, { art: true })}</div>` : ""}
    ${r.takes.length ? `<h3 class="mu-h">Recordings</h3><div class="mu-list">${r.takes.map((t) => `<div class="mu-row" data-take="${esc(t.id)}" tabindex="0" role="button"><div class="mu-art sm mu-glyph"><span>●</span></div><div class="grow"><div class="mu-t1">${esc(t.title)}</div><div class="mu-t2">${esc(t.session)} · ${esc(t.scene)}</div></div><span class="mu-dur mono">${muTime(t.duration)}</span></div>`).join("")}</div>` : ""}
    ${r.streams.length ? `<h3 class="mu-h">Radio</h3><div class="mu-list">${r.streams.map((s) => `<div class="mu-row" data-stream="${s.id}" tabindex="0" role="button"><div class="mu-art sm mu-glyph"><span>≋</span></div><div class="grow"><div class="mu-t1">${esc(s.name)}</div></div></div>`).join("")}</div>` : ""}`;
  $$("[data-artist]", el).forEach((row) => row.onclick = () => { MU.q = ""; $("#mu-q").value = ""; MU.tab = "artists"; muOpen("artist", row.dataset.artist); });
  $$("[data-album]", el).forEach((t) => { t.onclick = () => { MU.q = ""; $("#mu-q").value = ""; MU.tab = "albums"; muOpen("album", t.dataset.album); }; });
  $$("[data-play-album]", el).forEach((b) => b.onclick = (e) => { e.stopPropagation(); muPlay({ kind: "album", id: b.dataset.playAlbum }); });
  const tl = $("#mu-s-tracks", el); if (tl) muBindTrackRows(tl, r.tracks, (i) => ({ kind: "tracks", ids: r.tracks.map((t) => t.id), start: i }));
  $$("[data-take]", el).forEach((row) => row.onclick = () => muPlay({ kind: "take", id: row.dataset.take }, false));
  $$("[data-stream]", el).forEach((row) => row.onclick = () => muPlay({ kind: "stream", id: row.dataset.stream }, false));
}

// ------------------------------------------------------------------ library sources
async function muSources() {
  const el = $("#mu-sources");
  if (!el) return;
  let r;
  try { r = await get("/music/sources"); } catch (e) { el.innerHTML = `<div class="card">${esc(e.message)}</div>`; return; }
  const sc = (MU.st && MU.st.scan) || {};
  const lib = (MU.st && MU.st.library) || {};
  el.innerHTML = `<div class="card mu-src"><div class="card-head"><h3>Library</h3><div class="spacer"></div><button class="btn ghost small" id="mu-scan" ${sc.running ? "disabled" : ""}>${sc.running ? "Scanning…" : "Scan now"}</button></div>
    <div class="muted small">${lib.albums ?? "–"} albums · ${lib.tracks ?? "–"} songs · ${lib.takes ?? "–"} recordings · ${lib.streams ?? "–"} streams${sc.last ? ` · last scan ${sc.last.seconds.toFixed(1)} s` : ""}${sc.error ? ` · <span class="badge caution">${esc(sc.error)}</span>` : ""}</div>
    <div class="tbl-scroll"><table class="tbl"><thead><tr><th>Source</th><th>Collection</th><th>Path</th><th class="num">Files</th><th>State</th></tr></thead><tbody>
    ${r.sources.map((s) => `<tr><td>${esc(s.name || s.id)}</td><td class="small">${esc(s.collection)}</td><td class="mono small">${esc(s.path)}</td><td class="num">${s.files ?? ""}</td><td>${s.state === "ok" ? `<span class="badge ok">ok</span>` : s.state === "offline" || !s.present ? `<span class="badge off">${s.optional ? "not connected" : "missing"}</span>` : `<span class="badge">${esc(s.state || "new")}</span>`}</td></tr>`).join("")}
    </tbody></table></div><div class="muted small">Read-only: files are never changed. Sources are set in ${esc(r.file)}. Editing tags, merging and duplicates come later.</div></div>`;
  $("#mu-scan", el).onclick = async () => { try { await post("/music/scan", {}); toast("Scanning the library"); setTimeout(muSources, 400); } catch (e) { fail(e); } };
}

// ------------------------------------------------------------------ now playing + queue
function muBar() {
  const el = $("#mu-bar");
  if (!el) return;
  const st = MU.st, p = (st && st.player) || { connected: false, state: "stop" };
  const cur = p.current || null;
  const playing = p.state === "play";
  const dur = p.duration || (cur && cur.duration) || 0;
  const vmax = (st && st.volume_max) || 80;
  if (!el.dataset.built) {
    el.dataset.built = "1";
    el.innerHTML = `<div class="mu-np">${muArt("", "", "sm")}<div class="mu-np-txt"><div class="mu-t1" id="mu-np-t"></div><div class="mu-t2" id="mu-np-s"></div></div></div>
      <div class="mu-transport">
        <div class="mu-btns">
          <button class="mu-ib" data-c="random" aria-label="Shuffle" aria-pressed="false" title="Shuffle">${muIcon("shuffle")}</button>
          <button class="mu-ib" data-c="prev" aria-label="Previous" title="Previous">${muIcon("prev")}</button>
          <button class="mu-ib mu-main" data-c="toggle" aria-label="Play" title="Play / pause">${muIcon("play")}</button>
          <button class="mu-ib" data-c="next" aria-label="Next" title="Next">${muIcon("next")}</button>
          <button class="mu-ib" data-c="repeat" aria-label="Repeat" aria-pressed="false" title="Repeat">${muIcon("repeat")}</button>
        </div>
        <div class="mu-seek"><span class="mono" id="mu-el">0:00</span><input type="range" id="mu-seek" min="0" max="1" step="1" value="0" aria-label="Position"><span class="mono" id="mu-du">0:00</span></div>
      </div>
      <div class="mu-right">
        <span class="mu-fmt muted small" id="mu-fmt"></span>
        <label class="mu-vol" title="Player volume (capped on this unit)">${muIcon("vol")}<input type="range" id="mu-vol" min="0" max="${vmax}" step="1" value="0" aria-label="Player volume"><output class="mono small" id="mu-vol-v"></output></label>
        <button class="mu-ib" id="mu-qbtn" aria-label="Queue" aria-expanded="${MU.queueOpen}" title="Queue">${muIcon("queue")}</button>
      </div>`;
    $$("[data-c]", el).forEach((b) => b.onclick = () => {
      const c = b.dataset.c, pl = MU.st && MU.st.player || {};
      if (c === "random") muCtl("random", pl.random ? 0 : 1);
      else if (c === "repeat") muCtl("repeat", pl.repeat ? 0 : 1);
      else muCtl(c);
    });
    const sk = $("#mu-seek", el);
    sk.addEventListener("input", () => { MU.seeking = true; $("#mu-el").textContent = muTime(+sk.value); });
    sk.addEventListener("change", () => { MU.seeking = false; muCtl("seek", +sk.value); });
    const vl = $("#mu-vol", el);
    vl.addEventListener("input", () => { $("#mu-vol-v").textContent = vl.value + "%"; MU.volBusy = performance.now(); debounce("mu-vol", () => muCtl("volume", +vl.value), 120); });
    $("#mu-qbtn", el).onclick = () => { MU.queueOpen = !MU.queueOpen; $("#mu-queue").classList.toggle("hidden", !MU.queueOpen); $("#mu-qbtn").setAttribute("aria-expanded", MU.queueOpen); if (MU.queueOpen) muQueue(); };
  }
  el.classList.toggle("off", !p.connected);
  const art = $(".mu-np .mu-art", el);
  const key = (cur && cur.art) || "";
  if (art.dataset.art !== key) {
    art.replaceWith(Object.assign(document.createElement("div"), { className: "mu-art sm", innerHTML: `<span>${cur && cur.kind === "stream" ? "≋" : cur && cur.kind === "take" ? "●" : "♪"}</span>` }));
    const na = $(".mu-np .mu-art", el);
    if (key) { na.dataset.art = key; muLoadArt(na); } else na.dataset.art = "";
  }
  $("#mu-np-t").textContent = !p.connected ? "Player not running" : cur ? cur.title : "Nothing playing";
  $("#mu-np-s").textContent = !p.connected ? (p.error || "") : cur ? (cur.live ? `On air: ${cur.live}` : cur.subtitle) : "Choose an album, song, recording or station";
  const main = $('[data-c="toggle"]', el);
  if (main.dataset.s !== String(playing)) { main.dataset.s = String(playing); main.innerHTML = muIcon(playing ? "pause" : "play"); }
  main.setAttribute("aria-label", playing ? "Pause" : "Play");
  $('[data-c="random"]', el).setAttribute("aria-pressed", !!p.random);
  $('[data-c="repeat"]', el).setAttribute("aria-pressed", !!p.repeat);
  const sk = $("#mu-seek", el);
  const live = cur && cur.stream;
  sk.disabled = !cur || live || !dur;
  if (!MU.seeking) {
    const el2 = (p.elapsed || 0) + (playing ? (performance.now() - MU.stAt) / 1000 : 0);
    sk.max = Math.max(1, Math.floor(dur)); sk.value = Math.min(Math.floor(el2), sk.max);
    $("#mu-el").textContent = muTime(el2);
    $("#mu-du").textContent = live ? "live" : muTime(dur);
    sk.style.setProperty("--pct", dur ? `${Math.min(100, (el2 / dur) * 100)}%` : "0%");
  }
  const vl = $("#mu-vol", el);
  vl.max = vmax;
  if (performance.now() - MU.volBusy > 1500 && p.volume >= 0) { vl.value = p.volume; $("#mu-vol-v").textContent = p.volume + "%"; }
  vl.disabled = !p.connected;
  $("#mu-fmt").textContent = p.audio ? muFmt(p.audio, st.graph_rate) : "";
  $("#mu-fmt").title = p.output_path || "";
}
function muFmt(audio, graph) {
  const [sr, bits] = audio.split(":");
  const k = (+sr / 1000).toString().replace(/\.0$/, "");
  const b = bits === "f" ? "float" : bits ? `${bits}-bit` : "";
  return `${k} kHz ${b}${+sr !== graph ? ` → ${graph / 1000} kHz` : ""}`;
}
function muMarkPlaying() {
  const cur = MU.st && MU.st.player && MU.st.player.current;
  $$(".mu-row.now").forEach((r) => r.classList.remove("now"));
  if (!cur) return;
  if (cur.kind === "track") $$(`.mu-row[data-track="${cur.ref}"]`).forEach((r) => r.classList.add("now"));
  if (cur.kind === "take") $$(`.mu-row[data-take="${CSS.escape(cur.ref)}"]`).forEach((r) => r.classList.add("now"));
  if (cur.kind === "stream" && cur.ref) $$(`.mu-row[data-stream="${cur.ref}"]`).forEach((r) => r.classList.add("now"));
}
async function muQueue() {
  const el = $("#mu-queue");
  if (!el) return;
  let r = { queue: [] };
  try { r = await get("/music/queue"); } catch (e) { el.innerHTML = `<div class="card">${esc(e.message)}</div>`; return; }
  const pos = MU.st && MU.st.player ? MU.st.player.pos : -1;
  el.innerHTML = `<div class="mu-q-head"><h3>Queue</h3><div class="spacer"></div><button class="btn ghost small" id="mu-q-clear" ${r.queue.length ? "" : "disabled"}>Clear</button><button class="btn ghost small" id="mu-q-close" aria-label="Close queue">✕</button></div>
    <div class="mu-list">${r.queue.map((q) => `<div class="mu-row ${q.pos === pos ? "now" : ""}" data-qpos="${q.pos}" tabindex="0" role="button">
      ${q.kind === "track" ? muArt(q.art, q.title, "sm") : `<div class="mu-art sm mu-glyph"><span>${q.kind === "take" ? "●" : "≋"}</span></div>`}
      <div class="grow"><div class="mu-t1">${esc(q.title)}</div><div class="mu-t2">${esc(q.live || q.subtitle)}</div></div>
      <button class="btn ghost mu-more" data-qup="${q.pos}" aria-label="Move up" ${q.pos ? "" : "disabled"}>↑</button>
      <button class="btn ghost mu-more" data-qrm="${q.pos}" aria-label="Remove ${esc(q.title)}">✕</button></div>`).join("") || `<div class="empty-state">The queue is empty.</div>`}</div>`;
  $("#mu-q-close", el).onclick = () => { MU.queueOpen = false; el.classList.add("hidden"); const b = $("#mu-qbtn"); if (b) b.setAttribute("aria-expanded", "false"); };
  $("#mu-q-clear", el).onclick = async () => { try { await api("DELETE", "/music/queue"); muQueue(); muPollNow(); } catch (e) { fail(e); } };
  $$("[data-qpos]", el).forEach((row) => row.onclick = async (e) => { if (e.target.closest("button")) return; try { await post(`/music/queue/${row.dataset.qpos}/play`, {}); muPollNow(); } catch (er) { fail(er); } });
  $$("[data-qrm]", el).forEach((b) => b.onclick = async () => { try { await api("DELETE", `/music/queue/${b.dataset.qrm}`); muQueue(); muPollNow(); } catch (er) { fail(er); } });
  $$("[data-qup]", el).forEach((b) => b.onclick = async () => { const i = +b.dataset.qup; try { await post("/music/queue/move", { from: i, to: i - 1 }); muQueue(); } catch (er) { fail(er); } });
  muHydrateArt(el);
}

let muInFlight = false, muLastKey = "";
async function muPollNow() {
  if (muInFlight) return;
  muInFlight = true;
  try {
    const st = await get("/music/status");
    MU.st = st; MU.stAt = performance.now();
    const cur = st.player && st.player.current;
    const key = `${cur ? cur.kind + cur.ref + cur.live : ""}|${st.player && st.player.length}|${st.player && st.player.pos}`;
    if (key !== muLastKey) { muLastKey = key; muMarkPlaying(); if (MU.queueOpen) muQueue(); }
    if (MU.sourcesOpen && st.scan && (st.scan.running || st.scan.finished !== (MU.lastScanDone || 0))) { MU.lastScanDone = st.scan.finished; muSources(); }
  } catch (e) { /* keep the last state */ }
  muInFlight = false;
  muBar();
}
function muStartPoll() {
  clearInterval(MU.poll);
  muPollNow();
  let n = 0;
  MU.poll = setInterval(() => {
    if (S.view !== "music" || !$("#mu-bar")) { clearInterval(MU.poll); return; }
    if (++n % 4 === 0 || document.visibilityState === "visible" && n % 2 === 0) muPollNow(); else muBar();
  }, 500);
}
