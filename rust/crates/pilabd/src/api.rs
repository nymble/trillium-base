// Copyright (C) 2026 Nymble
// SPDX-License-Identifier: GPL-3.0-or-later
//! HTTP + WebSocket API (`/api/v1`) and the embedded web console.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pilab_core::scene::Slot;
use pilab_core::{Route, Scene, Settings};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::assets;
use crate::brand::{API_PREFIX, PRODUCT_NAME, PRODUCT_TAGLINE, VERSION};
use crate::paths::ct_eq;
use crate::record;
use crate::state::App;

type S = State<Arc<App>>;
type R = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn bad(e: impl ToString) -> (StatusCode, Json<Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": e.to_string() })))
}

fn ok(v: Value) -> R {
    Ok(Json(v))
}

pub const ENDPOINTS: &[(&str, &str, &str)] = &[
    ("GET", "/api/v1/info", "Product, version, device id, pairing requirement (no token needed)"),
    ("POST", "/api/v1/pair/check", "200 if the bearer token is valid"),
    ("GET", "/api/v1/ws?token=", "WebSocket: status frames at 15 Hz, sys frames every 2 s; send {t:'midi',bytes:[..]}"),
    ("GET", "/api/v1/status", "Voices, peaks, limiter, master, xruns, recording"),
    ("GET", "/api/v1/system", "Machine state, DAC, backend, services, ports, USB, events, recording and archive status"),
    ("POST", "/api/v1/services/{id}", "{on:bool} for services the console controls (pilab-synth)"),
    ("GET", "/api/v1/scene", "Active scene (JSON) and whether it has unsaved changes"),
    ("PUT", "/api/v1/scene", "Replace the active scene (applied live, not saved)"),
    ("PATCH", "/api/v1/scene", "{master_db?, a4_hz?, bend_range?, description?}"),
    ("PUT", "/api/v1/scene/slots/{id}", "Create or replace sound slot id (1..16)"),
    ("DELETE", "/api/v1/scene/slots/{id}", "Remove a slot and routes that target it"),
    ("GET", "/api/v1/scene/slots/{id}/wave", "One-cycle preview of the slot's oscillator"),
    ("PUT", "/api/v1/scene/routes", "Replace the MIDI routing map"),
    ("GET", "/api/v1/scenes", "Saved scenes"),
    ("GET", "/api/v1/scenes/{name}", "Saved scene as TOML"),
    ("POST", "/api/v1/scenes/{name}/load", "Load a saved scene (never raises the level)"),
    ("POST", "/api/v1/scenes/{name}/save", "Save the active scene under a name"),
    ("POST", "/api/v1/scenes/{name}/duplicate", "{to} copy a saved scene"),
    ("DELETE", "/api/v1/scenes/{name}", "Delete a saved scene"),
    ("POST", "/api/v1/midi", "{bytes:[..]} or {note,vel,on,ch} / {bend} / {cc,value}; target 'route' (default) or 'sound'"),
    ("POST", "/api/v1/audition", "{slot} sound played by target 'sound' (0 = none)"),
    ("GET", "/api/v1/versions", "console version and commit, OS, kernel, PipeWire, firmware, bootloader, model"),
    ("GET", "/api/v1/storage", "disks and partitions (lsblk) with use and temperature"),
    ("GET", "/api/v1/audio/devices", "audio outputs (sinks) and inputs (sources), AirPlay speakers marked output-only"),
    ("POST", "/api/v1/audition/play", "{slot|bank|table|preset} play a ~1.5 s phrase on the DAC at the current master"),
    ("GET", "/api/v1/audition/render?slot=|bank=|table=|preset=", "the same phrase rendered offline as a WAV for the browser"),
    ("GET", "/api/v1/library/samples", "installed sample banks (samples/index.json)"),
    ("GET", "/api/v1/library/presets", "named presets (sampler and oscillator)"),
    ("POST", "/api/v1/scene/slots/{id}/preset", "{preset} or {bank}: make the slot a sampler with that sound"),
    ("POST", "/api/v1/engine", "{run:bool} fade the synth in or out"),
    ("POST", "/api/v1/panic", "All sound off"),
    ("POST", "/api/v1/master", "{db} master level, capped at -6 dBFS"),
    ("GET", "/api/v1/midi/inputs", "MIDI input slots and their sources"),
    ("GET", "/api/v1/library/akwf", "AKWF groups"),
    ("GET", "/api/v1/library/akwf/{group}", "Files in a group"),
    ("GET", "/api/v1/library/preview?path=&points=", "Raw single cycle for drawing"),
    ("GET", "/api/v1/graph", "Audio/MIDI ports and links (PipeWire via the JACK API)"),
    ("POST", "/api/v1/graph/connect", "{from,to} link two ports"),
    ("POST", "/api/v1/graph/disconnect", "{from,to} unlink two ports"),
    ("GET", "/api/v1/settings", "Settings"),
    ("PUT", "/api/v1/settings", "Replace settings (archive 'configured' is not settable here)"),
    ("POST", "/api/v1/record", "{on:bool} start/stop local recording"),
    ("GET", "/api/v1/recordings", "Recording cards, newest first"),
    ("GET", "/api/v1/endpoints", "This list"),
];

pub fn router(app: Arc<App>) -> Router {
    let api = Router::new()
        .route("/info", get(info))
        .route("/pair/check", post(|| async { Json(json!({ "ok": true })) }))
        .route("/ws", get(ws))
        .route("/status", get(status))
        .route("/system", get(system))
        .route("/services/{id}", post(service_toggle))
        .route("/versions", get(versions))
        .route("/storage", get(storage))
        .route("/audio/devices", get(audio_devices))
        .route("/scene", get(scene_get).put(scene_put).patch(scene_patch))
        .route("/scene/slots/{id}", put(slot_put).delete(slot_delete))
        .route("/scene/slots/{id}/wave", get(slot_wave))
        .route("/scene/routes", put(routes_put))
        .route("/scenes", get(scenes_list))
        .route("/scenes/{name}", get(scene_toml).delete(scene_delete))
        .route("/scenes/{name}/load", post(scene_load))
        .route("/scenes/{name}/save", post(scene_save))
        .route("/scenes/{name}/duplicate", post(scene_dup))
        .route("/midi", post(midi))
        .route("/midi/inputs", get(midi_inputs))
        .route("/audition", post(audition))
        .route("/audition/play", post(audition_play))
        .route("/audition/render", get(audition_render))
        .route("/library/samples", get(samples_list))
        .route("/library/presets", get(presets_list))
        .route("/scene/slots/{id}/preset", post(slot_preset))
        .route("/engine", post(engine))
        .route("/panic", post(panic))
        .route("/master", post(master))
        .route("/library/akwf", get(akwf_groups))
        .route("/library/akwf/{group}", get(akwf_files))
        .route("/library/preview", get(akwf_preview))
        .route("/graph", get(graph))
        .route("/graph/connect", post(graph_connect))
        .route("/graph/disconnect", post(graph_disconnect))
        .route("/settings", get(settings_get).put(settings_put))
        .route("/record", post(record_toggle))
        .route("/recordings", get(recordings))
        .route("/endpoints", get(endpoints))
        .route_layer(middleware::from_fn_with_state(app.clone(), auth));
    Router::new().nest(API_PREFIX, api).fallback(static_file).with_state(app)
}

async fn auth(State(app): S, req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    if path.ends_with("/info") || path.ends_with("/endpoints") {
        return next.run(req).await;
    }
    let token = app.token.read().unwrap().clone();
    let mut okay = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .is_some_and(|t| ct_eq(t.trim(), &token));
    if !okay && path.ends_with("/ws") {
        if let Some(q) = req.uri().query() {
            okay = q.split('&').filter_map(|kv| kv.strip_prefix("token=")).any(|t| ct_eq(t, &token));
        }
    }
    if okay {
        next.run(req).await
    } else {
        (StatusCode::UNAUTHORIZED, Json(json!({ "error": "pairing token required", "hint": "run `pilabctl pair` on the Pi" }))).into_response()
    }
}

async fn info(State(app): S) -> Json<Value> {
    Json(json!({
        "product": PRODUCT_NAME,
        "tagline": PRODUCT_TAGLINE,
        "version": VERSION,
        "device_id": app.device_id,
        "hostname": app.hostname,
        "api": API_PREFIX,
        "ws": format!("{API_PREFIX}/ws"),
        "pairing_required": true,
        "pairing": "token",
        "audio_backend": app.backend.name(),
    }))
}

async fn endpoints() -> Json<Value> {
    Json(Value::Array(ENDPOINTS.iter().map(|(m, p, d)| json!({ "method": m, "path": p, "what": d })).collect()))
}

async fn ws(State(app): S, up: WebSocketUpgrade) -> Response {
    up.on_upgrade(move |sock| ws_session(app, sock))
}

async fn ws_session(app: Arc<App>, mut sock: WebSocket) {
    let mut rx = app.tx.subscribe();
    let first = app.system_json().to_string();
    if sock.send(Message::Text(first.into())).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            m = rx.recv() => match m {
                Ok(s) => { if sock.send(Message::Text(s.into())).await.is_err() { break; } }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            },
            m = sock.recv() => match m {
                Some(Ok(Message::Text(t))) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&t) {
                        if v["t"] == "midi" {
                            if let Ok(b) = serde_json::from_value::<MidiBody>(v.clone()) {
                                if let Ok(bytes) = encode(&b) {
                                    let _ = app.console_midi_on(port_for(b.target.as_deref()), &bytes);
                                }
                            }
                        }
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            }
        }
    }
}

async fn status(State(app): S) -> R {
    ok(app.status_json())
}
async fn system(State(app): S) -> R {
    ok(app.system_json())
}

#[derive(Deserialize)]
struct On {
    on: bool,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SvcBody {
    on: bool,
    confirm: String,
}

async fn service_toggle(State(app): S, Path(id): Path<String>, Json(b): Json<SvcBody>) -> R {
    if id == "pilab-synth" {
        app.engine_run(b.on).map_err(bad)?;
        return ok(json!({ "id": id, "on": b.on }));
    }
    let (id2, on, confirm) = (id.clone(), b.on, b.confirm.clone());
    let r = tokio::task::spawn_blocking(move || crate::sys::service_switch(&id2, on, &confirm)).await.map_err(|e| bad(e.to_string()))?;
    match r {
        Ok(msg) => {
            app.event("info", format!("service {id} {}: {msg}", if on { "on" } else { "off" }));
            app.refresh_services();
            ok(json!({ "id": id, "on": on, "message": msg }))
        }
        Err(e) => {
            app.event("warn", format!("service {id} {}: {e}", if on { "on" } else { "off" }));
            Err((StatusCode::CONFLICT, Json(json!({ "error": e }))))
        }
    }
}

async fn versions(State(app): S) -> R {
    let mut v = tokio::task::spawn_blocking(crate::sys::versions).await.map_err(|e| bad(e.to_string()))?;
    v["console"] = json!({ "name": PRODUCT_NAME, "version": VERSION, "commit": crate::brand::COMMIT });
    v["device_id"] = json!(app.device_id);
    ok(v)
}

async fn storage() -> R {
    ok(tokio::task::spawn_blocking(crate::sys::storage).await.map_err(|e| bad(e.to_string()))?)
}

async fn audio_devices() -> R {
    ok(tokio::task::spawn_blocking(crate::sys::audio_devices).await.map_err(|e| bad(e.to_string()))?)
}

async fn scene_get(State(app): S) -> R {
    let (sc, dirty) = app.scene();
    ok(json!({ "scene": sc, "dirty": dirty, "midi_inputs": app.midi_inputs_json(), "devices": app.device_classes_json() }))
}

async fn scene_put(State(app): S, Json(sc): Json<Scene>) -> R {
    app.replace_scene(sc).map_err(bad)?;
    scene_get(State(app)).await
}

#[derive(Deserialize)]
struct ScenePatch {
    master_db: Option<f32>,
    a4_hz: Option<f32>,
    bend_range: Option<f32>,
    description: Option<String>,
}

async fn scene_patch(State(app): S, Json(p): Json<ScenePatch>) -> R {
    app.edit_scene(|sc| {
        if let Some(v) = p.master_db {
            sc.master_db = v.clamp(-80.0, pilab_synth::engine::MAX_MASTER_DB);
        }
        if let Some(v) = p.a4_hz {
            sc.a4_hz = v;
        }
        if let Some(v) = p.bend_range {
            sc.bend_range = v.clamp(0.0, 24.0);
        }
        if let Some(v) = p.description {
            sc.description = v;
        }
        Ok(())
    })
    .map_err(bad)?;
    scene_get(State(app)).await
}

async fn slot_put(State(app): S, Path(id): Path<u8>, Json(mut slot): Json<Slot>) -> R {
    slot.id = id;
    app.upsert_slot(slot).map_err(bad)?;
    scene_get(State(app)).await
}

async fn slot_delete(State(app): S, Path(id): Path<u8>) -> R {
    app.delete_slot(id).map_err(bad)?;
    scene_get(State(app)).await
}

#[derive(Deserialize)]
struct Points {
    points: Option<usize>,
}

async fn slot_wave(State(app): S, Path(id): Path<u8>, Query(q): Query<Points>) -> R {
    ok(json!({ "points": app.slot_preview(id, q.points.unwrap_or(256).clamp(16, 1024)) }))
}

async fn routes_put(State(app): S, Json(routes): Json<Vec<Route>>) -> R {
    app.set_routes(routes).map_err(bad)?;
    scene_get(State(app)).await
}

async fn scenes_list(State(app): S) -> R {
    ok(json!({ "scenes": app.store.list(), "active": app.scene().0.name }))
}

async fn scene_toml(State(app): S, Path(name): Path<String>) -> Result<Response, (StatusCode, Json<Value>)> {
    let sc = app.store.load(&name).map_err(bad)?;
    let body = sc.to_toml().map_err(bad)?;
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response())
}

async fn scene_load(State(app): S, Path(name): Path<String>) -> R {
    app.load_scene(&name).map_err(bad)?;
    scene_get(State(app)).await
}

async fn scene_save(State(app): S, Path(name): Path<String>) -> R {
    app.save_scene(&name).map_err(bad)?;
    scenes_list(State(app)).await
}

#[derive(Deserialize)]
struct To {
    to: String,
}

async fn scene_dup(State(app): S, Path(name): Path<String>, Json(t): Json<To>) -> R {
    app.store.duplicate(&name, &t.to).map_err(bad)?;
    scenes_list(State(app)).await
}

async fn scene_delete(State(app): S, Path(name): Path<String>) -> R {
    if name == app.scene().0.name {
        return Err(bad("cannot delete the active scene; load another first"));
    }
    app.store.delete(&name).map_err(bad)?;
    scenes_list(State(app)).await
}

/// Either raw bytes, or a note / bend / cc described in fields (encoded with zpod-midi).
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct MidiBody {
    bytes: Option<Vec<u8>>,
    note: Option<u8>,
    vel: Option<u8>,
    on: Option<bool>,
    bend: Option<i32>,
    cc: Option<u8>,
    value: Option<u8>,
    ch: Option<u8>,
    /// "route" (default) plays through the MIDI map as the console input;
    /// "sound" plays the slot selected with /audition.
    target: Option<String>,
}

pub fn encode(b: &MidiBody) -> Result<Vec<u8>, String> {
    use zpod_midi::Msg;
    if let Some(bytes) = &b.bytes {
        return Ok(bytes.clone());
    }
    let ch = b.ch.unwrap_or(1).clamp(1, 16);
    let m = if let Some(n) = b.note {
        if b.on.unwrap_or(true) {
            Msg::note_on(ch, n, b.vel.unwrap_or(90))
        } else {
            Msg::note_off(ch, n)
        }
    } else if let Some(v) = b.bend {
        Msg::bend(ch, (v + 8192).clamp(0, 16383) as u16)
    } else if let Some(c) = b.cc {
        Msg::cc(ch, c, b.value.unwrap_or(0))
    } else {
        return Err("bytes, note, bend or cc required".into());
    };
    Ok(m.b[..m.len as usize].to_vec())
}

pub fn port_for(target: Option<&str>) -> u8 {
    if target == Some("sound") {
        pilab_synth::PORT_AUDITION
    } else {
        pilab_synth::PORT_CONSOLE
    }
}

async fn midi(State(app): S, Json(b): Json<MidiBody>) -> R {
    let bytes = encode(&b).map_err(bad)?;
    app.console_midi_on(port_for(b.target.as_deref()), &bytes).map_err(bad)?;
    ok(json!({ "ok": true, "bytes": bytes }))
}

#[derive(Deserialize)]
struct Aud {
    slot: u8,
}

async fn audition(State(app): S, Json(a): Json<Aud>) -> R {
    app.set_audition(a.slot);
    ok(json!({ "slot": a.slot }))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct AudSrcQ {
    slot: Option<u8>,
    bank: Option<String>,
    table: Option<String>,
    preset: Option<String>,
}

impl AudSrcQ {
    fn src(self) -> Result<crate::state::AuditionSrc, String> {
        use crate::state::AuditionSrc as A;
        if let Some(s) = self.slot {
            Ok(A::Slot(s))
        } else if let Some(b) = self.bank {
            Ok(A::Bank(b))
        } else if let Some(t) = self.table {
            Ok(A::Table(t))
        } else if let Some(p) = self.preset {
            Ok(A::Preset(p))
        } else {
            Err("slot, bank, table or preset required".into())
        }
    }
}

async fn audition_play(State(app): S, Json(q): Json<AudSrcQ>) -> R {
    let src = q.src().map_err(bad)?;
    let a = app.clone();
    let v = tokio::task::spawn_blocking(move || a.audition_sound_preload(&src).map(|_| src))
        .await
        .map_err(|e| bad(e.to_string()))?
        .map_err(bad)?;
    ok(app.audition_play(v).map_err(bad)?)
}

async fn audition_render(State(app): S, Query(q): Query<AudSrcQ>) -> Result<Response, (StatusCode, Json<Value>)> {
    let src = q.src().map_err(bad)?;
    let wav = tokio::task::spawn_blocking(move || app.audition_render(src)).await.map_err(|e| bad(e.to_string()))?.map_err(bad)?;
    Ok(([(header::CONTENT_TYPE, "audio/wav"), (header::CACHE_CONTROL, "no-store")], wav).into_response())
}

async fn samples_list(State(app): S) -> R {
    let banks: Vec<Value> = app
        .samples
        .banks()
        .into_iter()
        .map(|b| {
            let loaded = app.samples.is_loaded(&b.id);
            let mut v = serde_json::to_value(&b).unwrap_or_default();
            v["loaded"] = json!(loaded);
            v
        })
        .collect();
    ok(json!({
        "root": app.samples.root.display().to_string(),
        "available": !banks.is_empty(),
        "loaded_mib": app.samples.loaded_bytes() as f64 / 1048576.0,
        "banks": banks,
        "hint": "install with: sh scripts/fetch-samples.sh",
    }))
}

async fn presets_list(State(app): S) -> R {
    let installed: Vec<String> = app.samples.banks().into_iter().map(|b| b.id).collect();
    let v: Vec<Value> = pilab_core::presets::all()
        .into_iter()
        .map(|p| {
            let mut v = serde_json::to_value(&p).unwrap_or_default();
            v["installed"] = json!(p.bank.is_empty() || installed.iter().any(|i| i == p.bank));
            v
        })
        .collect();
    ok(json!({ "presets": v }))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct PresetBody {
    preset: Option<String>,
    bank: Option<String>,
    /// Add a channel-10 route for drum sounds when the slot has no route yet.
    route: Option<bool>,
}

async fn slot_preset(State(app): S, Path(id): Path<u8>, Json(b): Json<PresetBody>) -> R {
    let a = app.clone();
    tokio::task::spawn_blocking(move || a.apply_preset(id, b.preset.as_deref(), b.bank.as_deref(), b.route.unwrap_or(true)))
        .await
        .map_err(|e| bad(e.to_string()))?
        .map_err(bad)?;
    scene_get(State(app)).await
}

async fn midi_inputs(State(app): S) -> R {
    ok(app.midi_inputs_json())
}

#[derive(Deserialize)]
struct Run {
    run: bool,
}

async fn engine(State(app): S, Json(b): Json<Run>) -> R {
    app.engine_run(b.run).map_err(bad)?;
    ok(json!({ "running": b.run }))
}

async fn panic(State(app): S) -> R {
    app.send(pilab_synth::Cmd::Panic).map_err(bad)?;
    app.event("info", "panic: all sound off");
    ok(json!({ "ok": true }))
}

#[derive(Deserialize)]
struct Db {
    db: f32,
}

async fn master(State(app): S, Json(b): Json<Db>) -> R {
    let db = app.set_master(b.db).map_err(bad)?;
    ok(json!({ "master_db": db }))
}

async fn akwf_groups(State(app): S) -> R {
    ok(json!({ "root": app.library.root.display().to_string(), "available": app.library.available(), "source": app.library.source_note(), "groups": app.library.groups() }))
}

async fn akwf_files(State(app): S, Path(group): Path<String>) -> R {
    ok(json!({ "files": app.library.files(&group).map_err(bad)? }))
}

#[derive(Deserialize)]
struct Prev {
    path: String,
    points: Option<usize>,
}

async fn akwf_preview(State(app): S, Query(q): Query<Prev>) -> R {
    ok(json!({ "points": app.library.preview(&q.path, q.points.unwrap_or(128).clamp(16, 1024)).map_err(bad)? }))
}

async fn graph(State(app): S) -> R {
    ok(json!({ "backend": app.backend.name(), "ports": app.backend.graph() }))
}

#[derive(Deserialize)]
struct Link {
    from: String,
    to: String,
}

async fn graph_connect(State(app): S, Json(l): Json<Link>) -> R {
    app.backend.connect(&l.from, &l.to).map_err(bad)?;
    app.event("info", format!("linked {} -> {}", l.from, l.to));
    ok(json!({ "ok": true }))
}

async fn graph_disconnect(State(app): S, Json(l): Json<Link>) -> R {
    app.backend.disconnect(&l.from, &l.to).map_err(bad)?;
    app.event("info", format!("unlinked {} -> {}", l.from, l.to));
    ok(json!({ "ok": true }))
}

async fn settings_get(State(app): S) -> R {
    let s = app.settings();
    let (state, text) = s.archive_status();
    ok(json!({ "settings": s, "archive_state": state, "archive_text": text }))
}

async fn settings_put(State(app): S, Json(s): Json<Settings>) -> R {
    app.put_settings(s).map_err(bad)?;
    settings_get(State(app)).await
}

async fn record_toggle(State(app): S, Json(b): Json<On>) -> R {
    ok(app.record(b.on).map_err(bad)?)
}

async fn recordings(State(app): S) -> R {
    ok(json!({ "location": app.recordings_dir().display().to_string(), "recordings": record::list(&app.recordings_dir()) }))
}

async fn static_file(uri: Uri) -> Response {
    let path = uri.path();
    let p = if path == "/" { "/index.html" } else { path };
    match assets::get(p) {
        Some((mime, body)) => {
            let cache = if p == "/index.html" { "no-cache" } else { "public, max-age=3600" };
            let body = if p == "/index.html" { Body::from(assets::index_html()) } else { Body::from(body) };
            ([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, cache)], body).into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}
