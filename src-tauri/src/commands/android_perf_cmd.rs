//! "Medir rendimiento" (GameNative tab): its own OS window, a background
//! recording thread (`tatu_android::perf::record`), and the saved sessions
//! (`android_perf.rs`). Every window command is `async` — a sync command
//! building a webview deadlocks on Windows (see `open_detail_window`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tatu_android::Adb;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::SharedState;
use crate::android::{self, GameNativeConfig};
use crate::android_perf::{self, PerfDevice, PerfGame, PerfSession, PerfSessionHeader};
use crate::android_perf_insights::{self, Insight};

pub const PERF_LABEL: &str = "perf";

/// The game the performance window is showing — managed state rather than a
/// URL query, same reasoning as `window_cmd::DetailTarget`.
#[derive(Default)]
pub struct PerfTarget(pub Mutex<Option<PerfGame>>);

/// The in-flight recording's stop flag; `None` when idle. One recording at a
/// time — there's one window and one device being watched.
#[derive(Default)]
pub struct PerfRecorder(pub Mutex<Option<Arc<AtomicBool>>>);

impl PerfRecorder {
    fn is_recording(&self) -> bool {
        self.0.lock().is_ok_and(|g| g.is_some())
    }

    /// Also called when the window closes (`lib.rs`), so no sampler thread
    /// outlives the window it reports to.
    pub fn stop(&self) {
        if let Ok(guard) = self.0.lock()
            && let Some(flag) = guard.as_ref()
        {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

/// Opens the window on a game, or retargets and focuses it. While recording
/// it only focuses: a session must be saved under the game it started on.
#[tauri::command]
pub async fn open_perf_window(
    app: AppHandle,
    source: String,
    id: u64,
    name: String,
) -> Result<(), String> {
    let recording = app.state::<PerfRecorder>().is_recording();
    if let Some(existing) = app.get_webview_window(PERF_LABEL) {
        if !recording {
            let game = PerfGame { source, id, name };
            *app.state::<PerfTarget>()
                .0
                .lock()
                .map_err(|e| e.to_string())? = Some(game.clone());
            existing
                .emit("perf-target-changed", game)
                .map_err(|e| e.to_string())?;
        }
        let _ = existing.unminimize();
        return existing.set_focus().map_err(|e| e.to_string());
    }
    *app.state::<PerfTarget>()
        .0
        .lock()
        .map_err(|e| e.to_string())? = Some(PerfGame { source, id, name });
    WebviewWindowBuilder::new(&app, PERF_LABEL, WebviewUrl::App("perf.html".into()))
        .title("Tatu — rendimiento")
        .inner_size(1180.0, 900.0)
        .min_inner_size(640.0, 480.0)
        .resizable(true)
        .center()
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn perf_target(target: State<'_, PerfTarget>) -> Option<PerfGame> {
    target.0.lock().ok()?.clone()
}

#[tauri::command]
pub fn perf_is_recording(recorder: State<'_, PerfRecorder>) -> bool {
    recorder.is_recording()
}

/// Starts recording the target game on `serial` and returns right away.
/// Emits `perf_sample` (`{sample, live}`) every second, then `perf_done`
/// (`{session, error}`) once stopped — the session is already saved by
/// then, so closing the window can never lose it.
#[tauri::command]
pub async fn perf_start(
    app: AppHandle,
    state: State<'_, SharedState>,
    target: State<'_, PerfTarget>,
    recorder: State<'_, PerfRecorder>,
    serial: String,
    model: String,
) -> Result<(), String> {
    let game = target
        .0
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or("No hay un juego elegido")?;
    let (adb_path, best) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        let key = android::config_key(&game.source, game.id);
        (
            s.android.adb_path.clone(),
            s.android.configs.get(&key).cloned(),
        )
    };
    // Tuning is "change one thing and measure again": a new run starts from
    // the previous run's config, so only the change needs typing. The first
    // run starts from the game's saved best config.
    let config = android_perf::list(&game.source, game.id)
        .ok()
        .and_then(|sessions| sessions.into_iter().find_map(|h| h.config))
        .or(best);
    let adb = Adb::locate(&adb_path).map_err(|e| e.to_string())?;

    let stop = Arc::new(AtomicBool::new(false));
    {
        let mut slot = recorder.0.lock().map_err(|e| e.to_string())?;
        if slot.is_some() {
            return Err("Ya hay una medición en curso".into());
        }
        *slot = Some(stop.clone());
    }

    let started_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default();
    std::thread::spawn(move || {
        let recording = tatu_android::perf::record(&adb, &serial, &stop, |sample, live| {
            let payload = serde_json::json!({ "sample": sample, "live": live });
            let _ = app.emit_to(PERF_LABEL, "perf_sample", payload);
        });
        let session = PerfSession {
            header: PerfSessionHeader {
                id: started_ms,
                game,
                label: String::new(),
                tdp_note: String::new(),
                device: PerfDevice { serial, model },
                config,
                summary: recording.summary,
                error: recording.error,
            },
            samples: recording.samples,
        };
        // Nothing recorded (stopped instantly, device gone at once): no file.
        let saved = if session.samples.is_empty() {
            Err(session
                .header
                .error
                .clone()
                .unwrap_or_else(|| "No se registró ninguna muestra".into()))
        } else {
            android_perf::save(&session).map(|()| session.header)
        };
        if let Ok(mut slot) = app.state::<PerfRecorder>().0.lock() {
            *slot = None;
        }
        let payload = match saved {
            Ok(header) => serde_json::json!({ "session": header }),
            Err(e) => serde_json::json!({ "error": e }),
        };
        let _ = app.emit_to(PERF_LABEL, "perf_done", payload);
    });
    Ok(())
}

/// Only raises the stop flag — the recording thread finishes the tick it's
/// in, saves, and reports through `perf_done`.
#[tauri::command]
pub fn perf_stop(recorder: State<'_, PerfRecorder>) {
    recorder.stop();
}

/// One full probe's raw output, for the diagnostics block: which sources
/// this device actually answers.
#[tauri::command]
pub async fn perf_probe(state: State<'_, SharedState>, serial: String) -> Result<String, String> {
    let adb_path = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.android.adb_path.clone()
    };
    tokio::task::spawn_blocking(move || {
        let adb = Adb::locate(&adb_path)?;
        tatu_android::perf::probe_raw(&adb, &serial)
    })
    .await
    .map_err(|e| format!("Task error: {e}"))?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn perf_list(source: String, id: u64) -> Result<Vec<PerfSessionHeader>, String> {
    android_perf::list(&source, id)
}

#[tauri::command]
pub fn perf_load(source: String, id: u64, session: u64) -> Result<PerfSession, String> {
    android_perf::load(&source, id, session)
}

/// "Ideas para mejorar" for a saved session — computed on every view, so
/// better rules apply to old recordings too.
#[tauri::command]
pub fn perf_insights(source: String, id: u64, session: u64) -> Result<Vec<Insight>, String> {
    let s = android_perf::load(&source, id, session)?;
    Ok(android_perf_insights::analyze(
        &s.samples,
        &s.header.summary,
        s.header.config.as_ref(),
    ))
}

/// The part of a session the user writes: its name, the TDP / performance
/// mode it ran at, and the GameNative config it used.
#[tauri::command]
pub fn perf_update(
    source: String,
    id: u64,
    session: u64,
    label: String,
    tdp_note: String,
    config: Option<GameNativeConfig>,
) -> Result<PerfSessionHeader, String> {
    let mut s = android_perf::load(&source, id, session)?;
    s.header.label = label.trim().to_string();
    s.header.tdp_note = tdp_note.trim().to_string();
    s.header.config = config.filter(|c| !c.is_empty());
    android_perf::save(&s)?;
    Ok(s.header)
}

#[tauri::command]
pub fn perf_delete(source: String, id: u64, session: u64) -> Result<(), String> {
    android_perf::delete(&source, id, session)
}
