//! A dedicated thread owning every bootstrapped [`FrameworkRuntime`].
//!
//! mlua's `Lua` is `!Send` (the `lua54` build has no `send` feature), so a
//! framework runtime can't live behind a Tauri `State<Mutex<…>>` shared across
//! the command threadpool. Instead one actor thread owns the runtimes (keyed by
//! app id) and the commands talk to it over a channel — only the `Sender`
//! crosses threads, and that *is* `Send`. The actor also keeps each runtime
//! resident between toggles: the framework holds the symbol table, allocations
//! and enabled hooks a later cheat depends on, so it must outlive any single
//! enable (exactly as CE keeps the table's Lua state loaded).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};
use std::thread;

use cheat_runtime::{FrameworkRuntime, MemRec, find_pid_by_exe, parse_framework_table};

/// A request to run one cheat's `[ENABLE]`/`[DISABLE]` Lua against a table's
/// bootstrapped runtime.
struct ToggleReq {
    app_id: String,
    exe: String,
    ct_path: PathBuf,
    memrec: MemRec,
    /// The Lua source to run — the cheat's enable block when `on`, else its
    /// disable block.
    src: String,
    on: bool,
    reply: Sender<Result<(), String>>,
}

/// Handle to the framework runtime actor. Lives in Tauri state.
pub struct FrameworkActor {
    tx: Mutex<Sender<ToggleReq>>,
}

impl FrameworkActor {
    /// Spawn the actor thread and return a handle.
    pub fn spawn() -> Self {
        let (tx, rx) = channel::<ToggleReq>();
        thread::Builder::new()
            .name("framework-runtime".into())
            .spawn(move || {
                // Owned only by this thread — a (re)launched game re-bootstraps
                // on the next toggle (see the pid check in `handle_toggle`).
                let mut runtimes: HashMap<String, FrameworkRuntime> = HashMap::new();
                while let Ok(req) = rx.recv() {
                    let reply = req.reply.clone();
                    let _ = reply.send(handle_toggle(&mut runtimes, req));
                }
            })
            .expect("spawn framework-runtime thread");
        Self { tx: Mutex::new(tx) }
    }

    /// Run a cheat's enable (`on = true`) or disable block, bootstrapping the
    /// table's runtime on first use. Blocks until the actor replies.
    #[allow(clippy::too_many_arguments)]
    pub fn toggle(
        &self,
        app_id: String,
        exe: String,
        ct_path: PathBuf,
        memrec: MemRec,
        src: String,
        on: bool,
    ) -> Result<(), String> {
        let (reply, rx) = channel();
        let req = ToggleReq {
            app_id,
            exe,
            ct_path,
            memrec,
            src,
            on,
            reply,
        };
        self.tx
            .lock()
            .map_err(|e| format!("framework actor poisoned: {e}"))?
            .send(req)
            .map_err(|_| "framework actor thread is gone".to_string())?;
        rx.recv()
            .map_err(|_| "framework actor dropped the reply".to_string())?
    }
}

/// Runs in the actor thread: ensure a fresh runtime for the table's process,
/// then run the cheat block.
fn handle_toggle(
    runtimes: &mut HashMap<String, FrameworkRuntime>,
    req: ToggleReq,
) -> Result<(), String> {
    let pid = find_pid_by_exe(&req.exe)
        .ok_or_else(|| format!("game process '{}' is not running; launch it first", req.exe))?;

    // (Re)bootstrap if we have no runtime for this table or the process changed.
    let needs_load = runtimes.get(&req.app_id).is_none_or(|rt| rt.pid() != pid);
    if needs_load {
        let xml = std::fs::read_to_string(&req.ct_path)
            .map_err(|e| format!("read {}: {e}", req.ct_path.display()))?;
        let table =
            parse_framework_table(&xml).ok_or("table is not a Lua framework table".to_string())?;
        let rt = FrameworkRuntime::load(pid, &table).map_err(|e| e.to_string())?;
        runtimes.insert(req.app_id.clone(), rt);
    }

    let rt = runtimes
        .get(&req.app_id)
        .expect("runtime present after load");
    let run = if req.on {
        rt.enable(&req.memrec, &req.src)
    } else {
        rt.disable(&req.memrec, &req.src)
    };
    run.map_err(|e| e.to_string())
}
