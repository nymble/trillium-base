// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
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
    ("POST", "/api/v1/pair/code", "New one-time pairing code (5 min, single use) with its URL and QR code, to pair another device"),
    ("GET", "/api/v1/midi/router", "MIDI router: routes, per-device profiles, live status, sources, destinations, engines"),
    ("PUT", "/api/v1/midi/router", "{routes, profiles} replace the router configuration (saved, applied live)"),
    ("POST", "/api/v1/midi/router/reset", "Back to one route: every input to the scene"),
    ("POST", "/api/v1/midi/router/routes", "{from, to, channels, to_channel, note_lo, note_hi, types, transpose, name} add a route"),
    ("PUT", "/api/v1/midi/router/routes/{id}", "Replace one route"),
    ("DELETE", "/api/v1/midi/router/routes/{id}", "Remove one route"),
    ("PUT", "/api/v1/midi/router/profiles", "{matches, enabled, channel, transpose, velocity_scale, velocity_fixed, block} add or replace a device profile"),
    ("DELETE", "/api/v1/midi/router/profiles?matches=GLOB", "Remove a device profile"),
    ("GET", "/api/v1/midi/monitor?since=SEQ&limit=N", "MIDI monitor: events with source, message and destinations (last 5000 kept)"),
    ("GET", "/api/v1/midi/monitor.csv", "MIDI monitor history as CSV"),
    ("DELETE", "/api/v1/midi/monitor", "Clear the MIDI monitor history"),
    ("GET", "/api/v1/engines", "FluidSynth and ZynAddSubFX: installed, running, sound; SoundFonts, Zyn instruments, GM programs"),
    ("POST", "/api/v1/engines/{id}/start|stop", "Start or stop an engine service"),
    ("POST", "/api/v1/engines/{id}/sound", "{file} SoundFont (.sf2) or ZynAddSubFX instrument (.xiz); restarts the engine"),
    ("POST", "/api/v1/engines/{id}/program", "{channel, program, bank?} program change to a running engine"),
    ("POST", "/api/v1/engines/{id}/scene", "Create (if missing) and load the engine's scene: general midi or zynaddsubfx"),
    ("POST", "/api/v1/pair/redeem", "{code} exchange a one-time pairing code (pilabctl pair --qr) for the token; no token needed"),
    ("GET", "/api/v1/ws?token=", "WebSocket: status frames at 15 Hz, sys frames every 2 s; send {t:'midi',bytes:[..]}"),
    ("GET", "/api/v1/status", "Voices, peaks, limiter, master, xruns, recording"),
    ("GET", "/api/v1/system", "Machine state, DAC, backend, services, ports, USB, events, recording and archive status"),
    ("POST", "/api/v1/services/{id}", "{on:bool, confirm?} switch a service (synth id pilab-synth, alias trillium-synth)"),
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
    ("GET", "/api/v1/mixer", "audio nodes with PipeWire volume and mute (also links the meters)"),
    ("POST", "/api/v1/mixer/{id}", "{name, volume?, mute?} set a node's volume (cubic 0..1, ramped, capped at 1.0) or mute"),
    ("GET", "/api/v1/meters", "peak dBFS per metered client since the last read"),
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
    ("GET", "/api/v1/library/akwf/{group}/frames?points=", "Every cycle in a group, decimated, for thumbnails and the table view"),
    ("GET", "/api/v1/library/preview?path=&points=", "Raw single cycle for drawing"),
    ("GET", "/api/v1/graph", "Audio/MIDI ports and links (PipeWire via the JACK API)"),
    ("POST", "/api/v1/graph/connect", "{from,to} link two ports"),
    ("POST", "/api/v1/graph/disconnect", "{from,to} unlink two ports"),
    ("GET", "/api/v1/settings", "Settings"),
    ("PUT", "/api/v1/settings", "Replace settings (archive 'configured' is not settable here)"),
    ("POST", "/api/v1/record", "{on:bool} start/stop local recording"),
    ("GET", "/api/v1/recordings", "Recording cards, newest first"),
    ("GET", "/api/v1/recordings/{id}/audio", "A recording's WAV (for playback in the browser)"),
    ("GET", "/api/v1/input", "Audio input strip: source glob, connected ports, gain, monitor (off by default), record"),
    ("POST", "/api/v1/input", "{source?, gain_db?, monitor?, record?, autoconnect?, confirm?} change the input strip; monitor on needs confirm"),
    ("GET", "/api/v1/devices", "USB and Bluetooth devices, and Trillium Zero controllers with identity, status, settings and stored profile"),
    ("POST", "/api/v1/devices/{key}/snapshot", "Re-read a Trillium Zero and store its settings as its profile (read-only towards the device)"),
    ("POST", "/api/v1/devices/{key}/reapply", "{confirm:true} send the stored profile's differing settings to the device, then read it back"),
    ("GET", "/api/v1/devices/{key}/telemetry?hz=&ms=", "Sensor frames from a short telemetry lease (read-only)"),
    ("POST", "/api/v1/devices/{key}/mic", "{on, confirm?} start or stop the 16 kHz microphone test stream (on needs confirm); stored in the profile"),
    ("POST", "/api/v1/measure/strike", "{timeout_s?, rehearse?} arm a strike-to-sound take (up to 60 s after GO): first Zero note-on, synth onset and acoustic onset; rehearse injects a note instead; writes ~/.local/share/pilab/measurements/strike-*.json"),
    ("GET", "/api/v1/measure/strike/cue", "What to do at the device for a strike take, from its sensors and settings"),
    ("GET", "/api/v1/zero/schema", "Trillium Zero mapping schema: 41 settings with names, ranges, defaults and groups, and GM drum names"),
    ("GET", "/api/v1/zero/presets", "Built-in and saved Trillium Zero mapping presets"),
    ("POST", "/api/v1/zero/presets", "{name, values?|key?, scene?} save a mapping preset (values or a device's live settings)"),
    ("DELETE", "/api/v1/zero/presets/{id}", "Delete a saved mapping preset"),
    ("POST", "/api/v1/devices/{key}/settings", "{values:{id|key: value}, save?} change Trillium Zero settings live (validated) and store them in its profile"),
    ("POST", "/api/v1/devices/{key}/preset", "{id, scene?} apply a mapping preset to the Zero (and load its scene unless scene:false), stored in its profile"),
    ("GET", "/api/v1/devices/{key}/export", "The Zero's live settings as a trill-preset file (Trillium app format)"),
    ("POST", "/api/v1/devices/{key}/import", "{preset: trill-preset JSON, save?} apply a Trillium app preset file"),
    ("POST", "/api/v1/devices/{key}/action", "{action: panic|recalibrate|led-test|restore-defaults|zero-orientation}"),
    ("POST", "/api/v1/devices/{key}/auto", "{manual} re-apply the stored profile automatically on connect (manual:false, default) or only by hand"),
    ("GET", "/api/v1/setup", "First-run steps done or skipped (docs/first-run.md)"),
    ("PUT", "/api/v1/setup", "{done:[..], skipped:[..], dismissed?} store first-run progress"),
    ("GET", "/api/v1/firmware?refresh=1", "Zero firmware versions, bootloader drives, images on the Base, latest published release"),
    ("POST", "/api/v1/firmware/check?name=FILE.uf2", "Upload (octet-stream) and check a firmware image; read-only towards the Zero"),
    ("POST", "/api/v1/firmware/update", "{key, name, confirm, allow_downgrade?} install a checked image (writes the Zero's flash)"),
    ("GET", "/api/v1/endpoints", "This list"),
];

pub fn router(app: Arc<App>) -> Router {
    let api = Router::new()
        .route("/info", get(info))
        .route("/pair/check", post(|| async { Json(json!({ "ok": true })) }))
        .route("/pair/redeem", post(pair_redeem))
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
        .route("/midi/router", get(router_get).put(router_put))
        .route("/midi/router/reset", post(router_reset))
        .route("/midi/router/routes", post(route_add))
        .route("/midi/router/routes/{id}", put(route_put).delete(route_delete))
        .route("/midi/router/profiles", put(profile_put).delete(profile_delete))
        .route("/midi/monitor", get(monitor_get).delete(monitor_clear))
        .route("/midi/monitor.csv", get(monitor_csv))
        .route("/engines", get(engines_get))
        .route("/engines/{id}/start", post(engine_start))
        .route("/engines/{id}/stop", post(engine_stop))
        .route("/engines/{id}/sound", post(engine_sound))
        .route("/engines/{id}/program", post(engine_program))
        .route("/engines/{id}/scene", post(engine_scene))
        .route("/pair/code", post(pair_code))
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
        .route("/library/akwf/{group}/frames", get(akwf_frames))
        .route("/library/preview", get(akwf_preview))
        .route("/graph", get(graph))
        .route("/mixer", get(mixer))
        .route("/mixer/{id}", post(mixer_set))
        .route("/meters", get(meters))
        .route("/graph/connect", post(graph_connect))
        .route("/graph/disconnect", post(graph_disconnect))
        .route("/settings", get(settings_get).put(settings_put))
        .route("/record", post(record_toggle))
        .route("/recordings", get(recordings))
        .route("/recordings/{id}/audio", get(recording_audio))
        .route("/input", get(input_get).post(input_set))
        .route("/devices", get(devices_list))
        .route("/devices/{key}/snapshot", post(device_snapshot))
        .route("/devices/{key}/reapply", post(device_reapply))
        .route("/devices/{key}/telemetry", get(device_telemetry))
        .route("/devices/{key}/mic", post(device_mic))
        .route("/measure/strike", post(measure_strike))
        .route("/measure/strike/cue", get(measure_cue))
        .route("/zero/schema", get(|| async { Json(pilab_core::zero_map::schema_json()) }))
        .route("/zero/presets", get(zero_presets).post(zero_preset_save))
        .route("/zero/presets/{id}", axum::routing::delete(zero_preset_delete))
        .route("/devices/{key}/settings", post(device_settings))
        .route("/devices/{key}/preset", post(device_preset))
        .route("/devices/{key}/export", get(device_export))
        .route("/devices/{key}/import", post(device_import))
        .route("/devices/{key}/action", post(device_action))
        .route("/devices/{key}/auto", post(device_auto))
        .route("/setup", get(setup_get).put(setup_put))
        .route("/firmware", get(firmware_status))
        .route("/firmware/check", post(firmware_check))
        .route("/firmware/update", post(firmware_update))
        .route("/endpoints", get(endpoints))
        .route_layer(middleware::from_fn_with_state(app.clone(), auth));
    let guard = Arc::new(crate::security::Guard::new(&app.hostname));
    Router::new()
        .nest(API_PREFIX, api)
        .fallback(static_file)
        .with_state(app)
        .layer(middleware::from_fn_with_state(guard, crate::security::guard))
}

async fn auth(State(app): S, req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    if path.ends_with("/info") || path.ends_with("/endpoints") || path.ends_with("/pair/redeem") {
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

/// Failed redeem attempts against the current code; five failures burn the code.
static PAIR_FAILS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[derive(Deserialize)]
struct PairRedeem {
    code: String,
}

fn normalize_code(c: &str) -> String {
    c.chars().filter(|ch| ch.is_ascii_alphanumeric()).map(|ch| ch.to_ascii_uppercase()).collect()
}

/// Exchange a one-time pairing code for the console token. The code is single use, expires
/// (pilabctl sets 5 minutes), and is destroyed after 5 wrong guesses. Each failure waits.
async fn pair_redeem(State(app): S, Json(r): Json<PairRedeem>) -> Response {
    use std::sync::atomic::Ordering::SeqCst;
    let path = app.paths.pair_code();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let stored = std::fs::read_to_string(&path).ok().and_then(|s| {
        let mut it = s.split_whitespace();
        Some((normalize_code(it.next()?), it.next()?.parse::<u64>().ok()?))
    });
    let deny = |msg: &'static str| (StatusCode::UNAUTHORIZED, Json(json!({ "error": msg, "hint": "run `trillium-base pair --qr` on the device for a new code" }))).into_response();
    let Some((code, expires)) = stored else {
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        return deny("no pairing code is active");
    };
    if now > expires {
        let _ = std::fs::remove_file(&path);
        return deny("pairing code expired");
    }
    if code.len() >= 8 && ct_eq(&normalize_code(&r.code), &code) {
        let _ = std::fs::remove_file(&path);
        PAIR_FAILS.store(0, SeqCst);
        app.event("info", "console paired with a one-time code");
        let token = app.token.read().unwrap().clone();
        return Json(json!({ "ok": true, "token": token })).into_response();
    }
    if PAIR_FAILS.fetch_add(1, SeqCst) + 1 >= 5 {
        let _ = std::fs::remove_file(&path);
        PAIR_FAILS.store(0, SeqCst);
        app.event("warn", "pairing code destroyed after 5 wrong attempts");
    }
    tokio::time::sleep(std::time::Duration::from_millis(700)).await;
    deny("pairing code not accepted")
}

async fn info(State(app): S) -> Json<Value> {
    Json(json!({
        "product": PRODUCT_NAME,
        "device": crate::brand::DEVICE_NAME,
        "tagline": PRODUCT_TAGLINE,
        "version": VERSION,
        "commit": crate::brand::COMMIT,
        "build": assets::build_id(),
        "device_id": app.device_id,
        "hostname": app.hostname,
        "api": API_PREFIX,
        "ws": format!("{API_PREFIX}/ws"),
        "pairing_required": true,
        "pairing": "token",
        "pairing_code": true,
        "audio_backend": app.backend.name(),
        "synth": crate::brand::SYNTH_NAME,
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
    // "trillium-synth" is accepted as an alias of the stable id
    let id = if id == "trillium-synth" { crate::brand::SYNTH_ID.to_string() } else { id };
    if id == crate::brand::SYNTH_ID {
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
    let a = app.clone();
    tokio::task::spawn_blocking(move || a.upsert_slot(slot)).await.map_err(|e| bad(e.to_string()))?.map_err(bad)?;
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

async fn akwf_frames(State(app): S, Path(group): Path<String>, Query(q): Query<Points>) -> R {
    let pts = q.points.unwrap_or(48).clamp(8, 256);
    let a = app.clone();
    let v = tokio::task::spawn_blocking(move || a.library.group_frames(&group, pts)).await.map_err(|e| bad(e.to_string()))?.map_err(bad)?;
    ok(json!({ "frames": v.into_iter().map(|(f, p)| json!({ "file": f, "points": p })).collect::<Vec<_>>() }))
}

async fn input_get(State(app): S) -> R {
    ok(tokio::task::spawn_blocking(move || app.input_json()).await.map_err(|e| bad(e.to_string()))?)
}

#[derive(Deserialize)]
struct InputSet {
    source: Option<String>,
    gain_db: Option<f32>,
    monitor: Option<bool>,
    record: Option<bool>,
    autoconnect: Option<bool>,
    #[serde(default)]
    confirm: bool,
}

async fn input_set(State(app): S, Json(b): Json<InputSet>) -> R {
    let v = tokio::task::spawn_blocking(move || app.set_input(b.source, b.gain_db, b.monitor, b.record, b.autoconnect, b.confirm)).await.map_err(|e| bad(e.to_string()))?;
    ok(v.map_err(bad)?)
}

async fn devices_list(State(app): S) -> R {
    let v = tokio::task::spawn_blocking(move || {
        let s = app.system_json();
        let mut zeros = app.devmgr.zeros_json();
        let underruns = app.zeromic.lock().ok().and_then(|g| g.as_ref().map(|b| b.dropped())).unwrap_or(0);
        for z in zeros.iter_mut() {
            z["mic_underruns"] = json!(underruns);
        }
        json!({
            "usb": s.get("usb").cloned().unwrap_or(Value::Null),
            "ble_connected": crate::identity::ble_connected_aliases(),
            "zeros": zeros,
            "midi_inputs": app.midi_inputs_json(),
        })
    })
    .await
    .map_err(|e| bad(e.to_string()))?;
    ok(v)
}

async fn device_snapshot(State(app): S, Path(key): Path<String>) -> R {
    let a = app.clone();
    let v = tokio::task::spawn_blocking(move || a.devmgr.snapshot(&key)).await.map_err(|e| bad(e.to_string()))?.map_err(bad)?;
    if let Some(ev) = v.get("events").and_then(|e| e.as_array()) {
        for t in ev.iter().filter_map(|t| t.as_str()) {
            app.event("info", t.to_string());
        }
    }
    ok(v)
}

#[derive(Deserialize, Default)]
struct Confirm {
    #[serde(default)]
    confirm: bool,
}

async fn device_reapply(State(app): S, Path(key): Path<String>, Json(b): Json<Confirm>) -> R {
    let a = app.clone();
    let k = key.clone();
    let v = tokio::task::spawn_blocking(move || a.devmgr.reapply(&k, b.confirm)).await.map_err(|e| bad(e.to_string()))?.map_err(bad)?;
    let n = v.get("changed").and_then(|c| c.as_array()).map(|c| c.len()).unwrap_or(0);
    let level = if n == 0 { "info" } else { "warn" };
    app.event(level, format!("Trillium Zero ({key}): re-applied {n} stored setting(s) from the console"));
    ok(v)
}

#[derive(Deserialize)]
struct TelemetryQ {
    hz: Option<u8>,
    ms: Option<u64>,
}

async fn device_telemetry(State(app): S, Path(key): Path<String>, Query(q): Query<TelemetryQ>) -> R {
    let v = tokio::task::spawn_blocking(move || app.devmgr.telemetry(&key, q.hz.unwrap_or(20), q.ms.unwrap_or(1000))).await.map_err(|e| bad(e.to_string()))?.map_err(bad)?;
    ok(json!({ "frames": v }))
}

#[derive(Deserialize)]
struct MicBody {
    on: bool,
    #[serde(default)]
    confirm: bool,
}

async fn device_mic(State(app): S, Path(key): Path<String>, Json(b): Json<MicBody>) -> R {
    let a = app.clone();
    let k = key.clone();
    let v = tokio::task::spawn_blocking(move || a.set_zero_mic(&k, b.on, b.confirm)).await.map_err(|e| bad(e.to_string()))?.map_err(bad)?;
    ok(v)
}

#[derive(Deserialize)]
struct StrikeBody {
    #[serde(default)]
    timeout_s: Option<u64>,
    /// Inject a note instead of waiting for a person (checks the measurement itself).
    #[serde(default)]
    rehearse: bool,
}

async fn measure_strike(State(app): S, body: Option<Json<StrikeBody>>) -> R {
    let (timeout, rehearse) = body.map(|Json(b)| (b.timeout_s.unwrap_or(60), b.rehearse)).unwrap_or((60, false));
    let v = tokio::task::spawn_blocking(move || app.measure_strike(timeout, rehearse)).await.map_err(|e| bad(e.to_string()))?.map_err(bad)?;
    ok(v)
}

async fn measure_cue(State(app): S) -> R {
    let plan = app.devmgr.strike_plan();
    ok(json!({ "cue": crate::measure::cue(plan.as_ref(), false), "plan": plan }))
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

async fn mixer(State(app): S) -> R {
    let a = app.clone();
    ok(tokio::task::spawn_blocking(move || a.mixer_json()).await.map_err(|e| bad(e.to_string()))?)
}

async fn meters(State(app): S) -> R {
    ok(app.meters_json())
}

#[derive(Deserialize)]
struct MixSet {
    name: String,
    volume: Option<f32>,
    mute: Option<bool>,
}

async fn mixer_set(State(app): S, Path(id): Path<u32>, Json(b): Json<MixSet>) -> R {
    let a = app.clone();
    let n = tokio::task::spawn_blocking(move || a.mixer.set(id, &b.name, b.volume, b.mute)).await.map_err(|e| bad(e.to_string()))?.map_err(bad)?;
    app.event("info", format!("{}: volume {} {}", n.label, n.volume.map(|v| format!("{:.0}% ({:.1} dB)", v * 100.0, crate::mixer::cubic_db(v))).unwrap_or("-".into()), if n.mute == Some(true) { "(muted)" } else { "" }));
    ok(json!(n))
}

async fn graph_connect(State(app): S, Json(mut l): Json<Link>) -> R {
    l.from = crate::audio::canon_port(&l.from);
    l.to = crate::audio::canon_port(&l.to);
    app.backend.connect(&l.from, &l.to).map_err(bad)?;
    app.event("info", format!("linked {} -> {}", l.from, l.to));
    ok(json!({ "ok": true }))
}

async fn graph_disconnect(State(app): S, Json(mut l): Json<Link>) -> R {
    l.from = crate::audio::canon_port(&l.from);
    l.to = crate::audio::canon_port(&l.to);
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

async fn recording_audio(State(app): S, Path(id): Path<String>) -> Result<Response, (StatusCode, Json<Value>)> {
    if !id.starts_with("pilab-rec-") || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        return Err(bad("bad recording id"));
    }
    let p = app.recordings_dir().join(&id).join(format!("{id}.wav"));
    let meta = std::fs::metadata(&p).map_err(|_| (StatusCode::NOT_FOUND, Json(json!({ "error": "no such recording" }))))?;
    if meta.len() > 512 * 1024 * 1024 {
        return Err(bad("recording is too large to play in the browser; copy it from the device"));
    }
    let body = tokio::task::spawn_blocking(move || std::fs::read(&p)).await.map_err(bad)?.map_err(bad)?;
    Ok(([(header::CONTENT_TYPE, "audio/wav"), (header::CACHE_CONTROL, "private, max-age=3600")], body).into_response())
}

async fn recordings(State(app): S) -> R {
    ok(json!({ "location": app.recordings_dir().display().to_string(), "recordings": record::list(&app.recordings_dir()) }))
}

/// Caching: index.html and unversioned URLs are `no-cache` with an ETag (the build id), so
/// browsers revalidate and get a 304 when nothing changed. URLs carrying `?v=<current
/// build>` (as written into index.html) are immutable for a year. A `?v=` naming another
/// build gets the current file with `no-cache`.
async fn static_file(uri: Uri, headers: axum::http::HeaderMap) -> Response {
    let path = uri.path();
    let p = if path == "/" { "/index.html" } else { path };
    let Some((mime, body)) = assets::get(p) else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    let build = assets::build_id();
    let versioned = p != "/index.html"
        && uri.query().is_some_and(|q| q.split('&').any(|kv| kv.strip_prefix("v=") == Some(build)));
    let etag = format!("\"{build}\"");
    if versioned {
        return ([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")], Body::from(body)).into_response();
    }
    let hdrs = [(header::CONTENT_TYPE, mime.to_string()), (header::CACHE_CONTROL, "no-cache".to_string()), (header::ETAG, etag.clone())];
    let matched = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim().trim_start_matches("W/") == etag));
    if matched {
        return (StatusCode::NOT_MODIFIED, hdrs).into_response();
    }
    let body = if p == "/index.html" { Body::from(assets::index_html()) } else { Body::from(body) };
    (hdrs, body).into_response()
}


// ------------------------------------------------------------------ Trillium Zero mapping

fn parse_values(v: &Value) -> Result<Vec<(u8, u16)>, String> {
    let obj = v.as_object().ok_or("values: an object of setting → value")?;
    let mut out = vec![];
    for (k, x) in obj {
        let spec = pilab_core::zero_map::lookup(k).ok_or_else(|| format!("unknown setting {k}"))?;
        let n = match x {
            Value::Number(n) => n.as_u64().and_then(|n| u16::try_from(n).ok()).ok_or_else(|| format!("{k}: bad number"))?,
            Value::String(t) => pilab_core::zero_map::parse_value(spec, t)?,
            Value::Bool(b) => *b as u16,
            _ => return Err(format!("{k}: number or text")),
        };
        pilab_core::zero_map::validate(spec.id, n)?;
        out.push((spec.id, n));
    }
    Ok(out)
}

async fn zero_presets(State(app): S) -> R {
    ok(json!({ "presets": app.devmgr.presets() }))
}

#[derive(Deserialize)]
struct PresetSave {
    name: String,
    #[serde(default)]
    values: Option<Value>,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    scene: String,
}

async fn zero_preset_save(State(app): S, Json(b): Json<PresetSave>) -> R {
    let values = match (&b.values, &b.key) {
        (Some(v), _) => parse_values(v).map_err(bad)?,
        (None, Some(k)) => app.devmgr.live_values(k),
        _ => return Err(bad("values or key")),
    };
    if values.is_empty() {
        return Err(bad("no settings to save"));
    }
    ok(app.devmgr.save_preset(&b.name, &values, &b.scene).map_err(bad)?)
}

async fn zero_preset_delete(State(app): S, Path(id): Path<String>) -> R {
    ok(app.devmgr.delete_preset(&id).map_err(bad)?)
}

#[derive(Deserialize)]
struct SettingsBody {
    values: Value,
    #[serde(default = "yes")]
    save: bool,
}
fn yes() -> bool {
    true
}

async fn device_settings(State(app): S, Path(key): Path<String>, Json(b): Json<SettingsBody>) -> R {
    let vals = parse_values(&b.values).map_err(bad)?;
    let a = app.clone();
    let k = key.clone();
    let v = tokio::task::spawn_blocking(move || a.devmgr.set_values(&k, &vals, b.save, None)).await.map_err(|e| bad(e.to_string()))?.map_err(bad)?;
    let n = v["sent"].as_array().map(|x| x.len()).unwrap_or(0);
    if n > 0 {
        app.event("info", format!("Trillium Zero ({key}): {n} setting(s) changed from the console"));
    }
    ok(v)
}

#[derive(Deserialize)]
struct PresetApply {
    id: String,
    #[serde(default = "yes")]
    scene: bool,
}

async fn device_preset(State(app): S, Path(key): Path<String>, Json(b): Json<PresetApply>) -> R {
    let p = app.devmgr.presets().into_iter().find(|p| p.id == b.id).ok_or_else(|| bad(format!("no preset {}", b.id)))?;
    let a = app.clone();
    let k = key.clone();
    let vals = p.values.clone();
    let name = p.name.clone();
    let mut v = tokio::task::spawn_blocking(move || a.devmgr.set_values(&k, &vals, true, Some(&name))).await.map_err(|e| bad(e.to_string()))?.map_err(bad)?;
    if b.scene && !p.scene.is_empty() {
        match app.load_scene(&p.scene) {
            Ok(_) => v["scene"] = json!(p.scene),
            Err(e) => v["scene_error"] = json!(e),
        }
    }
    app.event("info", format!("Trillium Zero ({key}): mapping \"{}\" applied{}", p.name, if b.scene && !p.scene.is_empty() { format!(", scene \"{}\" loaded", p.scene) } else { String::new() }));
    ok(v)
}

async fn device_export(State(app): S, Path(key): Path<String>) -> R {
    let vals = app.devmgr.live_values(&key);
    if vals.is_empty() {
        return Err(bad("no settings read from this device yet"));
    }
    ok(pilab_core::zero_map::to_trill_preset(&key, &format!("Trillium Base {key}"), &vals))
}

#[derive(Deserialize)]
struct ImportBody {
    preset: Value,
    #[serde(default = "yes")]
    save: bool,
}

async fn device_import(State(app): S, Path(key): Path<String>, Json(b): Json<ImportBody>) -> R {
    let (name, vals) = pilab_core::zero_map::from_trill_preset(&b.preset).map_err(bad)?;
    let a = app.clone();
    let k = key.clone();
    let n2 = name.clone();
    let mut v = tokio::task::spawn_blocking(move || a.devmgr.set_values(&k, &vals, b.save, Some(&n2))).await.map_err(|e| bad(e.to_string()))?.map_err(bad)?;
    v["name"] = json!(name);
    ok(v)
}

#[derive(Deserialize)]
struct ActionBody {
    action: String,
}

async fn device_action(State(app): S, Path(key): Path<String>, Json(b): Json<ActionBody>) -> R {
    let a = app.clone();
    let v = tokio::task::spawn_blocking(move || a.devmgr.action(&key, &b.action)).await.map_err(|e| bad(e.to_string()))?.map_err(bad)?;
    ok(v)
}

#[derive(Deserialize)]
struct AutoBody {
    manual: bool,
}

async fn device_auto(State(app): S, Path(key): Path<String>, Json(b): Json<AutoBody>) -> R {
    ok(app.devmgr.set_manual(&key, b.manual).map_err(bad)?)
}

// ------------------------------------------------------------------ firmware

#[derive(Deserialize)]
struct FwQ {
    #[serde(default)]
    refresh: Option<u8>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

async fn firmware_status(State(app): S, Query(q): Query<FwQ>) -> R {
    let a = app.clone();
    let refresh = q.refresh.unwrap_or(0) == 1;
    let v = tokio::task::spawn_blocking(move || {
        if refresh {
            crate::firmware::latest(&a.paths.data, true);
        }
        crate::firmware::status(&a.devmgr, &a.paths.data)
    })
    .await
    .map_err(|e| bad(e.to_string()))?;
    ok(v)
}

async fn firmware_check(State(app): S, Query(q): Query<FwQ>, body: axum::body::Bytes) -> R {
    let a = app.clone();
    let v = tokio::task::spawn_blocking(move || match (q.path, q.name) {
        (Some(p), _) if body.is_empty() => crate::firmware::check_path(&a.devmgr, std::path::Path::new(&p)),
        (_, Some(n)) => crate::firmware::check_upload(&a.devmgr, &a.paths.data, &n, &body),
        _ => Err("name=FILE.uf2 with the image as the body, or path= for a file on the Base".into()),
    })
    .await
    .map_err(|e| bad(e.to_string()))?
    .map_err(bad)?;
    ok(v)
}

#[derive(Deserialize)]
struct FwUpdate {
    key: String,
    name: String,
    #[serde(default)]
    confirm: bool,
    #[serde(default)]
    allow_downgrade: bool,
}

async fn firmware_update(State(app): S, Json(b): Json<FwUpdate>) -> R {
    let a = app.clone();
    app.event("warn", format!("Trillium Zero ({}): firmware update to {} requested", b.key, b.name));
    let v = tokio::task::spawn_blocking(move || crate::firmware::update(&a.devmgr, &a.paths.data, &b.key, &b.name, b.confirm, b.allow_downgrade)).await.map_err(|e| bad(e.to_string()))?.map_err(bad)?;
    app.event("info", format!("Trillium Zero firmware update: {}", v));
    ok(v)
}


// ------------------------------------------------------------------ first-run setup progress

const SETUP_IDS: &[&str] = &["pair", "name", "sound", "play", "access", "save"];

async fn setup_get(State(app): S) -> R {
    let p = app.paths.config.join("setup.json");
    let v = std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()).unwrap_or_else(|| json!({ "done": [], "skipped": [] }));
    ok(v)
}

async fn setup_put(State(app): S, Json(b): Json<Value>) -> R {
    let pick = |k: &str| -> Vec<String> { b[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).filter(|x| SETUP_IDS.contains(x)).map(str::to_string).collect()).unwrap_or_default() };
    let v = json!({ "done": pick("done"), "skipped": pick("skipped"), "dismissed": b["dismissed"].as_bool().unwrap_or(false) });
    crate::paths::write_atomic(&app.paths.config.join("setup.json"), serde_json::to_string_pretty(&v).unwrap().as_bytes()).map_err(bad)?;
    ok(v)
}


// ------------------------------------------------------------------ MIDI router, monitor, engines

async fn router_get(State(app): S) -> R {
    app.router_refresh(false);
    ok(app.router_json())
}

fn router_save(app: &App, c: pilab_core::midi_router::RouterConfig) -> R {
    app.router.set_config(c).map_err(bad)?;
    app.router_refresh(true);
    ok(app.router_json())
}

async fn router_put(State(app): S, Json(mut c): Json<pilab_core::midi_router::RouterConfig>) -> R {
    c.format = "pilab-midi-router".into();
    c.version = 1;
    router_save(&app, c)
}

async fn router_reset(State(app): S) -> R {
    router_save(&app, Default::default())
}

async fn route_add(State(app): S, Json(mut r): Json<pilab_core::midi_router::RouteCfg>) -> R {
    let mut c = app.router.config();
    if r.id.is_empty() || c.routes.iter().any(|x| x.id == r.id) {
        let base = if r.name.is_empty() { format!("{}-to-{}", r.from, r.to) } else { r.name.clone() };
        r.id = c.new_id(&base.replace(['*', ' ', '/', ':'], "-"));
    }
    c.routes.push(r);
    router_save(&app, c)
}

async fn route_put(State(app): S, Path(id): Path<String>, Json(mut r): Json<pilab_core::midi_router::RouteCfg>) -> R {
    let mut c = app.router.config();
    let i = c.routes.iter().position(|x| x.id == id).ok_or_else(|| bad("no such route"))?;
    r.id = id;
    c.routes[i] = r;
    router_save(&app, c)
}

async fn route_delete(State(app): S, Path(id): Path<String>) -> R {
    let mut c = app.router.config();
    let n = c.routes.len();
    c.routes.retain(|x| x.id != id);
    if c.routes.len() == n {
        return Err(bad("no such route"));
    }
    router_save(&app, c)
}

async fn profile_put(State(app): S, Json(p): Json<pilab_core::midi_router::Profile>) -> R {
    let mut c = app.router.config();
    match c.profiles.iter().position(|x| x.matches == p.matches) {
        Some(i) => c.profiles[i] = p,
        None => c.profiles.push(p),
    }
    router_save(&app, c)
}

#[derive(Deserialize)]
struct MatchQ {
    matches: String,
}

async fn profile_delete(State(app): S, Query(q): Query<MatchQ>) -> R {
    let mut c = app.router.config();
    c.profiles.retain(|x| x.matches != q.matches);
    router_save(&app, c)
}

#[derive(Deserialize)]
struct MonQ {
    #[serde(default)]
    since: u64,
    #[serde(default)]
    limit: Option<usize>,
}

async fn monitor_get(State(app): S, Query(q): Query<MonQ>) -> R {
    let ev = app.router.since(q.since, q.limit.unwrap_or(500).min(5000));
    let last = ev.last().map(|e| e.seq).unwrap_or(q.since);
    ok(json!({ "events": ev, "last": last, "kept": crate::router::HISTORY }))
}

async fn monitor_clear(State(app): S) -> R {
    app.router.clear();
    ok(json!({ "ok": true }))
}

async fn monitor_csv(State(app): S) -> Response {
    let name = format!("midi-monitor-{}.csv", crate::timefmt::iso_local(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)).replace(':', "").replace('T', "-"));
    (
        [(axum::http::header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()), (axum::http::header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))],
        app.router.csv(),
    )
        .into_response()
}

async fn engines_get(State(app): S) -> R {
    let sinks = app.backend.midi_sinks();
    let running = crate::router::Router::engine_ports(&sinks);
    ok(json!({
        "engines": crate::engines::status(&app.paths.config, &running),
        "soundfonts": crate::engines::soundfonts(),
        "zyn_instruments": crate::engines::zyn_instruments(),
        "gm_programs": crate::engines::GM_PROGRAMS.to_vec(),
    }))
}

async fn engine_start(State(app): S, Path(id): Path<String>) -> R {
    crate::engines::start(&id).map_err(bad)?;
    app.event("info", format!("engine {id} started"));
    ok(json!({ "ok": true }))
}

async fn engine_stop(State(app): S, Path(id): Path<String>) -> R {
    crate::engines::stop(&id).map_err(bad)?;
    app.event("info", format!("engine {id} stopped"));
    app.router_refresh(true);
    ok(json!({ "ok": true }))
}

#[derive(Deserialize)]
struct SoundB {
    file: String,
}

async fn engine_sound(State(app): S, Path(id): Path<String>, Json(b): Json<SoundB>) -> R {
    crate::engines::set_sound(&app.paths.config, &id, &b.file).map_err(bad)?;
    app.event("info", format!("engine {id}: sound {}", b.file.rsplit('/').next().unwrap_or("")));
    ok(json!({ "ok": true }))
}

#[derive(Deserialize)]
struct ProgB {
    channel: u8,
    program: u8,
    #[serde(default)]
    bank: Option<u8>,
}

async fn engine_program(State(app): S, Path(id): Path<String>, Json(b): Json<ProgB>) -> R {
    if !(1..=16).contains(&b.channel) || b.program > 127 {
        return Err(bad("channel 1-16, program 0-127"));
    }
    app.router_refresh(false);
    let ch = b.channel - 1;
    if let Some(bank) = b.bank {
        app.router.send_to_engine(&id, &[0xB0 | ch, 0, bank.min(127)]).map_err(bad)?;
    }
    app.router.send_to_engine(&id, &[0xC0 | ch, b.program]).map_err(bad)?;
    ok(json!({ "ok": true }))
}

async fn engine_scene(State(app): S, Path(id): Path<String>) -> R {
    let sc = match id.as_str() {
        "fluidsynth" => pilab_core::Scene::general_midi(),
        "zynaddsubfx" => pilab_core::Scene::zynaddsubfx(),
        _ => return Err(bad("unknown engine")),
    };
    if app.store.load(&sc.name).is_err() {
        app.store.save(&sc, &sc.name).map_err(bad)?;
    }
    let applied = app.load_scene(&sc.name).map_err(bad)?;
    ok(json!({ "ok": true, "scene": applied.name }))
}

/// A one-time pairing code for another phone, tablet or computer (same rules as
/// `trillium-base pair --qr`: 5 minutes, single use, destroyed after 5 wrong guesses).
async fn pair_code(State(app): S) -> R {
    use std::io::{Read, Write};
    const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
    const TTL: u64 = 300;
    let mut buf = [0u8; 8];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)).map_err(bad)?;
    let code: String = buf.iter().map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char).collect();
    let shown = format!("{}-{}", &code[..4], &code[4..]);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let path = app.paths.pair_code();
    let tmp = path.with_extension("tmp");
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).map_err(bad)?;
        writeln!(f, "{code} {}", now + TTL).map_err(bad)?;
    }
    std::fs::rename(&tmp, &path).map_err(bad)?;
    let url = format!("http://{}.local:{}/#pair={shown}", app.hostname, app.port);
    let svg = std::process::Command::new("qrencode").args(["-t", "svg", "-m", "2", "-o", "-", &url]).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).to_string());
    app.event("info", "pairing code created for another device");
    ok(json!({ "code": shown, "url": url, "expires": now + TTL, "ttl": TTL, "qr_svg": svg }))
}
