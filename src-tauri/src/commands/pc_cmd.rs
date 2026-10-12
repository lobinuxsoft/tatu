//! The PC tab: what upscalers an installed game ships, OptiScaler in and out
//! of it — every game file it replaces is backed up inside the game folder
//! and revert puts it back — DLSS DLL swaps the same way, and on Linux the
//! Steam launch options those need.
//!
//! Every command is async and does its file-system work off the main thread;
//! the app state lock is only held to copy a game's entry out
//! (`pc::snapshot`) or to record a launch-option change.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tatu_upscalers::settings::{self, Settings};
use tatu_upscalers::{IniOption, UpscalerError, detect, official, optiscaler, package, patch};
use tauri::{AppHandle, Emitter, State};

use crate::SharedState;
use crate::pc::{self, LaunchChange};

/// The PC tab's downloads (OptiScaler's release, a vendor's runtime DLL):
/// each with the stop signal its own Cancel button reaches by token, and the
/// files it is fetching. Several can run at once — a second download of the
/// *same* file is refused, they'd write the same half-downloaded copy.
#[derive(Default)]
pub struct Downloads(Mutex<Running>);

#[derive(Default)]
struct Running {
    /// The frontend's token for one download → its stop signal.
    by_token: HashMap<String, Arc<AtomicBool>>,
    /// Files being downloaded right now, by any of them.
    files: HashSet<String>,
}

/// One download; its token and files are released when dropped.
struct Download {
    downloads: Arc<Downloads>,
    token: String,
    files: Vec<String>,
    flag: Arc<AtomicBool>,
}

impl Downloads {
    /// A download the tab can cancel by `token`, before knowing what it will
    /// fetch ([`Download::reserve`]). Held by the work it belongs to, which
    /// outlives the command's borrow of the state.
    fn claim(self: &Arc<Self>, token: &str) -> Result<Download, String> {
        let mut running = self.0.lock().map_err(|e| e.to_string())?;
        let flag = Arc::new(AtomicBool::new(false));
        running.by_token.insert(token.to_string(), flag.clone());
        Ok(Download {
            downloads: Arc::clone(self),
            token: token.to_string(),
            files: Vec::new(),
            flag,
        })
    }

    fn cancel(&self, token: &str) {
        if let Ok(running) = self.0.lock()
            && let Some(flag) = running.by_token.get(token)
        {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

impl Download {
    /// Take the files this download will fetch, or refuse: another one is
    /// already fetching one of them.
    fn reserve(&mut self, files: &[&str]) -> Result<(), String> {
        let mut running = self.downloads.0.lock().map_err(|e| e.to_string())?;
        if let Some(busy) = files.iter().find(|name| running.files.contains(**name)) {
            return Err(format!("Ya se está descargando {busy}"));
        }
        for name in files {
            running.files.insert((*name).to_string());
            self.files.push((*name).to_string());
        }
        Ok(())
    }
}

impl Drop for Download {
    fn drop(&mut self) {
        if let Ok(mut running) = self.downloads.0.lock() {
            running.by_token.remove(&self.token);
            for name in &self.files {
                running.files.remove(name);
            }
        }
    }
}

/// Game folders with an install, revert or swap in progress. One at a time
/// per game: two at once race on the same manifest and backups (two detail
/// windows, a double click).
#[derive(Default)]
pub struct PcBusy(Mutex<HashSet<PathBuf>>);

struct BusyGuard<'a> {
    busy: &'a PcBusy,
    root: PathBuf,
}

impl PcBusy {
    fn claim(&self, root: &Path) -> Result<BusyGuard<'_>, String> {
        let mut set = self.0.lock().map_err(|e| e.to_string())?;
        if !set.insert(root.to_path_buf()) {
            return Err("Ya hay una operación en curso en este juego: esperá a que termine".into());
        }
        Ok(BusyGuard {
            busy: self,
            root: root.to_path_buf(),
        })
    }
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut set) = self.busy.0.lock() {
            set.remove(&self.root);
        }
    }
}

/// GE-Proton's switches the tab offers: newer DLSS DLLs (and the latest
/// preset), FSR 3.1 → FSR 4 on RDNA4 / RDNA3. Valve's Proton has none of them.
const PROTON_UPGRADES: [&str; 3] = [
    "PROTON_DLSS_UPGRADE",
    "PROTON_FSR4_UPGRADE",
    "PROTON_FSR4_RDNA3_UPGRADE",
];

/// Proxy names Proton already loads native — DXVK's dxgi, vkd3d-proton's
/// d3d12 — so a copy next to the exe wins without any override.
#[cfg(unix)]
const PROTON_NATIVE: [&str; 2] = ["dxgi.dll", "d3d12.dll"];

#[derive(Serialize)]
pub struct PatchView {
    id: String,
    state: patch::PatchState,
    meta: serde_json::Value,
    /// The game's files this patch replaced, kept in its backup (for a patch
    /// in conflict: the ones still pending).
    backed_up: Vec<String>,
    files: usize,
}

impl From<&patch::Manifest> for PatchView {
    fn from(m: &patch::Manifest) -> Self {
        PatchView {
            id: m.id.clone(),
            state: m.state,
            meta: m.meta.clone(),
            backed_up: m.backed_up(),
            files: m.entries.len(),
        }
    }
}

#[derive(Serialize)]
pub struct PcDetection {
    root: String,
    exe: Option<String>,
    /// Folder OptiScaler goes into, under `root` (`""` = the root itself).
    target: String,
    target_custom: bool,
    scan: detect::Scan,
    proxies: Vec<detect::Proxy>,
    default_proxy: Option<&'static str>,
    foreign_optiscaler: Option<&'static str>,
    /// An old OptiScaler under a name Tatu doesn't use (nvngx.dll, .asi).
    legacy_optiscaler: Option<&'static str>,
    /// Present GPUs, the one games run on first.
    gpus: Vec<tatu_upscalers::gpu::Gpu>,
    /// Some GPU runs DLSS (NVIDIA on NVIDIA's driver): OptiScaler keeps
    /// DLSS's own inputs, the DLSS switches apply.
    nvidia: bool,
    optiscaler: Option<PatchView>,
    /// Every other patch (DLSS / FSR / XeSS updates and swaps).
    patches: Vec<PatchView>,
    /// The game's DLLs its vendor's newest official release is newer than —
    /// DLSS ones only on a GPU that runs DLSS.
    official_updates: Vec<official::Update>,
    /// Patch folders whose record can't be read.
    broken_patches: Vec<String>,
    package_version: &'static str,
    linux: Option<LinuxInfo>,
}

#[derive(Serialize, Default)]
#[cfg_attr(not(unix), allow(dead_code))]
pub struct LinuxInfo {
    /// A Steam game: Tatu can edit its launch options. Anything else, the
    /// user pastes what's needed into their launcher.
    steam: bool,
    compat_tool: Option<String>,
    ge_proton: bool,
    upgrades: BTreeMap<&'static str, bool>,
    /// GE-Proton's own OptiScaler is on: installing ours would load two.
    proton_optiscaler: bool,
    /// Tatu's own change to these launch options — the active account's, or
    /// one left on another account.
    launch_change: Option<LaunchChange>,
    /// Why the launch options couldn't be read (installing waits for them:
    /// GE-Proton's own OptiScaler might be on).
    launch_options_error: Option<String>,
    /// The installed OptiScaler's DLL as the launch options list it right
    /// now (e.g. `n,b`), when its name needs an override: what's actually in
    /// effect, whoever wrote it.
    proxy_override: Option<String>,
}

#[derive(Serialize)]
pub struct LaunchReport {
    /// `not_needed` · `applied` · `removed` · `already` · `steam_running` ·
    /// `conflict` · `manual` · `error`
    status: &'static str,
    detail: String,
}

impl LaunchReport {
    fn new(status: &'static str, detail: impl Into<String>) -> Self {
        LaunchReport {
            status,
            detail: detail.into(),
        }
    }
}

#[derive(Serialize)]
pub struct InstallReport {
    patch: PatchView,
    launch: LaunchReport,
}

#[derive(Serialize)]
pub struct RevertOutcome {
    report: patch::RevertReport,
    launch: LaunchReport,
}

#[derive(Serialize, Clone)]
struct Progress {
    done: u64,
    total: u64,
}

#[derive(Deserialize)]
pub struct InstallRequest {
    #[serde(flatten)]
    opts: optiscaler::InstallOptions,
    /// The user read the anti-cheat warning and plays this game offline.
    #[serde(default)]
    accept_anticheat: bool,
}

#[derive(Clone)]
struct Game {
    root: PathBuf,
    exe: Option<String>,
    target: String,
    target_custom: bool,
    /// Launch options Tatu can edit: Steam games only.
    #[cfg_attr(not(unix), allow(dead_code))]
    steam_app: Option<String>,
    #[cfg_attr(not(unix), allow(dead_code))]
    launch_changes: Vec<LaunchChange>,
    /// The game's exes: none of them may be running while its files change.
    running: Vec<String>,
}

/// The game's entry copied out under the lock, then its folders and exes
/// resolved without it.
async fn game(shared: &SharedState, source: &str, id: u64) -> Result<Game, String> {
    let snap = {
        let state = shared.lock().map_err(|e| e.to_string())?;
        pc::snapshot(&state, source, id)?
    };
    let steam_app = (source == "steam").then(|| id.to_string());
    blocking(move || {
        let root = crate::installed::folder(&snap.game)?;
        let exe = pc::exe_rel(&snap, &root);
        let (target, target_custom) = pc::target_rel(&snap, &root, exe.as_deref());
        let running = patch::game_exes(&root, exe.as_deref(), &[target.as_str()]);
        Ok(Game {
            root,
            exe,
            target,
            target_custom,
            steam_app,
            launch_changes: snap.launch_changes,
            running,
        })
    })
    .await
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
}

fn refuse_while_running(g: &Game) -> Result<(), String> {
    if patch::is_running(&g.running) {
        return Err(UpscalerError::GameRunning.to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn pc_detect(
    state: State<'_, SharedState>,
    source: String,
    id: u64,
) -> Result<PcDetection, String> {
    let g = game(&state, &source, id).await?;
    blocking(move || Ok(detect_blocking(&g))).await
}

fn detect_blocking(g: &Game) -> PcDetection {
    let proxies = detect::proxies(&g.root, &g.target);
    let foreign = detect::foreign_optiscaler(&proxies);
    let default_proxy =
        foreign.or_else(|| proxies.iter().find(|p| p.taken.is_none()).map(|p| p.name));
    let manifests = patch::list(&g.root);
    let optiscaler = manifests.iter().find(|m| m.id == optiscaler::PATCH_ID);
    let proxy = optiscaler.and_then(|m| m.meta["proxy"].as_str());
    let gpus = tatu_upscalers::gpu::gpus();
    let scan = detect::scan(&g.root);
    let runs_dlss = gpus.iter().any(|gpu| gpu.dlss);
    let official_updates = scan
        .upscalers
        .iter()
        .filter(|u| u.patched_by.is_none())
        .filter_map(|u| official::update_for(&u.rel, u.version.as_deref()))
        .filter(|update| update.sdk != official::Sdk::Dlss || runs_dlss)
        .collect();
    PcDetection {
        root: g.root.display().to_string(),
        exe: g.exe.clone(),
        target: g.target.clone(),
        target_custom: g.target_custom,
        scan,
        official_updates,
        default_proxy,
        foreign_optiscaler: foreign,
        legacy_optiscaler: detect::legacy_optiscaler(&g.root, &g.target),
        proxies,
        nvidia: gpus.iter().any(|gpu| gpu.dlss),
        gpus,
        linux: linux_info(g, proxy),
        optiscaler: optiscaler.map(PatchView::from),
        patches: manifests
            .iter()
            .filter(|m| m.id != optiscaler::PATCH_ID)
            .map(PatchView::from)
            .collect(),
        broken_patches: patch::broken(&g.root),
        package_version: package::VERSION,
    }
}

#[cfg(unix)]
fn linux_info(g: &Game, proxy: Option<&str>) -> Option<LinuxInfo> {
    let account = crate::steam::current_account();
    let mut info = LinuxInfo {
        launch_change: account
            .as_ref()
            .ok()
            .and_then(|a| g.launch_changes.iter().find(|c| c.account == *a))
            .or(g.launch_changes.first())
            .cloned(),
        ..Default::default()
    };
    if let Some(app) = &g.steam_app {
        let options = match account.and_then(|a| crate::steam::read_app_launch_options(a, app)) {
            Ok(options) => options,
            Err(e) => {
                info.launch_options_error = Some(launch_error_text(&e));
                String::new()
            }
        };
        let on = |key| crate::steam::get_env(&options, key).is_some_and(|v| v != "0");
        info.steam = true;
        info.compat_tool = crate::steam::compat_tool(app);
        info.ge_proton = info.compat_tool.as_deref().is_some_and(|t| {
            let t = t.to_lowercase();
            t.contains("ge-proton") || t.contains("proton-ge")
        });
        for key in PROTON_UPGRADES {
            info.upgrades.insert(key, on(key));
        }
        info.proton_optiscaler = on("PROTON_USE_OPTISCALER");
        info.proxy_override = proxy
            .filter(|p| !PROTON_NATIVE.contains(p))
            .and_then(|p| crate::steam::override_mode(&options, p.trim_end_matches(".dll")));
    }
    Some(info)
}

#[cfg(not(unix))]
fn linux_info(_: &Game, _: Option<&str>) -> Option<LinuxInfo> {
    None
}

/// Download (once, shared by every game) and extract the pinned OptiScaler
/// release; answers which upscaler and frame generation codes its ini
/// documents.
#[tauri::command]
pub async fn optiscaler_prepare(
    app: AppHandle,
    downloads: State<'_, Arc<Downloads>>,
    cancel_token: String,
) -> Result<BTreeMap<&'static str, Vec<IniOption>>, String> {
    let mut download = downloads.claim(&cancel_token)?;
    download.reserve(&["OptiScaler"])?;
    let flag = download.flag.clone();
    blocking(move || {
        let files = package::ensure(
            &pc::cache_dir(),
            |done, total| {
                let _ = app.emit("optiscaler_progress", Progress { done, total });
            },
            &flag,
        )
        .map_err(|e| e.to_string())?;
        Ok(optiscaler::options(&files, None))
    })
    .await
}

/// Stop the download the tab started under `token`.
#[tauri::command]
pub fn pc_cancel_download(downloads: State<'_, Arc<Downloads>>, token: String) {
    downloads.cancel(&token);
}

/// The upscaler codes for a custom OptiScaler build: from the OptiScaler.ini
/// next to it, else the release's.
#[tauri::command]
pub async fn optiscaler_custom_options(
    dll: String,
) -> Result<BTreeMap<&'static str, Vec<IniOption>>, String> {
    blocking(move || {
        let dll = PathBuf::from(dll);
        if !tatu_upscalers::is_optiscaler(&dll) {
            return Err(format!("{} no es un OptiScaler.dll", dll.display()));
        }
        let files =
            package::cached(&pc::cache_dir()).ok_or("Primero hay que descargar OptiScaler")?;
        Ok(optiscaler::options(&files, Some(&dll)))
    })
    .await
}

#[tauri::command]
pub async fn optiscaler_install(
    state: State<'_, SharedState>,
    busy: State<'_, PcBusy>,
    source: String,
    id: u64,
    request: InstallRequest,
) -> Result<InstallReport, String> {
    let g = game(&state, &source, id).await?;
    let _busy = busy.claim(&g.root)?;
    let staging = pc::cache_dir()
        .join("staging")
        .join(format!("{source}_{id}"));
    let job = g.clone();
    let manifest = blocking(move || {
        let result = install_blocking(&job, &request, &staging);
        let _ = std::fs::remove_dir_all(&staging);
        result
    })
    .await?;
    Ok(InstallReport {
        patch: PatchView::from(&manifest),
        launch: launch_sync(&state, &g).await,
    })
}

fn install_blocking(
    g: &Game,
    request: &InstallRequest,
    staging: &Path,
) -> Result<patch::Manifest, String> {
    // Fails closed: unread launch options could hide GE-Proton's own
    // OptiScaler, and two of them in one game don't load.
    #[cfg(unix)]
    if let Some(app) = &g.steam_app {
        let options = crate::steam::current_account()
            .and_then(|account| crate::steam::read_app_launch_options(account, app))
            .map_err(|e| {
                format!(
                    "No pude leer las opciones de lanzamiento de Steam para ver si GE-Proton ya carga su propio \
                     OptiScaler: {}",
                    launch_error_text(&e)
                )
            })?;
        if crate::steam::get_env(&options, "PROTON_USE_OPTISCALER").is_some_and(|v| v != "0") {
            return Err("Este juego ya usa el OptiScaler de GE-Proton (PROTON_USE_OPTISCALER en las opciones de \
                        lanzamiento): instalar otro lo cargaría dos veces"
                .into());
        }
    }
    refuse_anticheat(&g.root, request.accept_anticheat)?;
    let files = package::cached(&pc::cache_dir()).ok_or("Primero hay que descargar OptiScaler")?;
    package::verify(&files).map_err(|e| e.to_string())?;
    let plan = optiscaler::plan(
        &g.root,
        &g.target,
        &files,
        &request.opts,
        staging,
        g.running.clone(),
    )
    .map_err(|e| e.to_string())?;
    patch::apply(&g.root, &plan).map_err(|e| e.to_string())
}

fn refuse_anticheat(root: &Path, accepted: bool) -> Result<(), String> {
    let anti_cheat = detect::scan(root).anti_cheat;
    if !anti_cheat.is_empty() && !accepted {
        return Err(format!(
            "Este juego trae anti-cheat ({}). Cambiarle DLLs puede terminar en un ban si jugás online",
            anti_cheat.join(", ")
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn optiscaler_revert(
    state: State<'_, SharedState>,
    busy: State<'_, PcBusy>,
    source: String,
    id: u64,
) -> Result<RevertOutcome, String> {
    let g = game(&state, &source, id).await?;
    let _busy = busy.claim(&g.root)?;
    refuse_while_running(&g)?;
    let root = g.root.clone();
    let (report, proxy) = blocking(move || {
        let proxy = patch::load(&root, optiscaler::PATCH_ID)
            .ok()
            .flatten()
            .and_then(|m| m.meta["proxy"].as_str().map(str::to_string));
        patch::revert(&root, optiscaler::PATCH_ID)
            .map(|report| (report, proxy))
            .map_err(|e| e.to_string())
    })
    .await?;
    // Files first, then the launch option: an override left behind with no
    // DLL is harmless (Wine falls back to the builtin), so it never blocks
    // the part that matters.
    let launch = launch_sync(&state, &g).await;
    Ok(RevertOutcome {
        launch: manual_removal_hint(&g, proxy.as_deref(), report.complete()).unwrap_or(launch),
        report,
    })
}

/// A non-Steam game on Linux: the override the user pasted into their
/// launcher is theirs to take out.
#[cfg(unix)]
fn manual_removal_hint(g: &Game, proxy: Option<&str>, reverted: bool) -> Option<LaunchReport> {
    let proxy = proxy.filter(|p| !PROTON_NATIVE.contains(p))?;
    (reverted && g.steam_app.is_none()).then(|| {
        LaunchReport::new(
            "manual",
            format!(
                "Ya podés quitar WINEDLLOVERRIDES=\"{}=n,b\" de las variables de entorno del lanzador del juego",
                proxy.trim_end_matches(".dll")
            ),
        )
    })
}

#[cfg(not(unix))]
fn manual_removal_hint(_: &Game, _: Option<&str>, _: bool) -> Option<LaunchReport> {
    None
}

/// Revert a DLSS swap. OptiScaler has its own command: its revert also puts
/// the launch options back.
#[tauri::command]
pub async fn pc_patch_revert(
    state: State<'_, SharedState>,
    busy: State<'_, PcBusy>,
    source: String,
    id: u64,
    patch_id: String,
) -> Result<patch::RevertReport, String> {
    let g = game(&state, &source, id).await?;
    let _busy = busy.claim(&g.root)?;
    if patch_id == optiscaler::PATCH_ID {
        return Err("OptiScaler se revierte con su propio botón".into());
    }
    refuse_while_running(&g)?;
    let root = g.root.clone();
    blocking(move || {
        require_patch(&root, &patch_id)?;
        if optiscaler::relies_on(&root, &patch_id) {
            return Err(
                "OptiScaler usa estos DLL (los instaló en la misma carpeta): revertí OptiScaler primero y \
                 después esta actualización"
                    .into(),
            );
        }
        patch::revert(&root, &patch_id).map_err(|e| e.to_string())
    })
    .await
}

/// A patch left in conflict after a game update: keep the game's current
/// files and drop the backups of the old ones.
#[tauri::command]
pub async fn pc_patch_keep_current(
    state: State<'_, SharedState>,
    busy: State<'_, PcBusy>,
    source: String,
    id: u64,
    patch_id: String,
) -> Result<LaunchReport, String> {
    let g = game(&state, &source, id).await?;
    let _busy = busy.claim(&g.root)?;
    let root = g.root.clone();
    let id_for_job = patch_id.clone();
    blocking(move || {
        require_patch(&root, &id_for_job)?;
        patch::keep_current(&root, &id_for_job).map_err(|e| e.to_string())
    })
    .await?;
    Ok(if patch_id == optiscaler::PATCH_ID {
        launch_sync(&state, &g).await
    } else {
        LaunchReport::new("not_needed", "")
    })
}

/// Only ids of patches actually recorded in this game get anywhere near the
/// file system.
fn require_patch(root: &Path, patch_id: &str) -> Result<(), String> {
    if patch::list(root).iter().any(|m| m.id == patch_id) {
        Ok(())
    } else {
        Err(UpscalerError::NotApplied(patch_id.to_string()).to_string())
    }
}

/// Replace one of the game's DLSS DLLs (nvngx_dlss / dlssd / dlssg) with a
/// newer copy the user already has. The game's own is backed up like any
/// patch.
#[tauri::command]
pub async fn dlss_swap(
    state: State<'_, SharedState>,
    busy: State<'_, PcBusy>,
    source: String,
    id: u64,
    rel: String,
    file: String,
    accept_anticheat: bool,
) -> Result<PatchView, String> {
    let g = game(&state, &source, id).await?;
    let _busy = busy.claim(&g.root)?;
    blocking(move || {
        refuse_anticheat(&g.root, accept_anticheat)?;
        swap_dlss(&g, &rel, &PathBuf::from(file))
    })
    .await
}

/// Replace one of the game's DLSS, FSR or XeSS DLLs with its vendor's
/// newest official release, downloaded from the vendor's SDK repository
/// first (once, shared by every game) — see `tatu_upscalers::official`. An
/// FSR 3.1 loader brings the providers it loads along. Everything it
/// replaces is backed up, as one patch.
// Tauri hands each managed state and argument in separately.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn runtime_update(
    app: AppHandle,
    state: State<'_, SharedState>,
    busy: State<'_, PcBusy>,
    downloads: State<'_, Arc<Downloads>>,
    source: String,
    id: u64,
    rel: String,
    accept_anticheat: bool,
    cancel_token: String,
) -> Result<PatchView, String> {
    // Cancel works from the first moment the tab shows it: finding the
    // game's folder and exes can take a while.
    let mut download = downloads.claim(&cancel_token)?;
    let flag = download.flag.clone();
    let g = game(&state, &source, id).await?;
    let _busy = busy.claim(&g.root)?;
    blocking(move || {
        // `rel` comes from the scan, as the game's folder spells it.
        let from_version = tatu_upscalers::file_version(&g.root.join(&rel));
        let update = official::update_for(&rel, from_version.as_deref())
            .ok_or("Este DLL ya es igual o más nuevo que la versión oficial")?;
        // NVIDIA's license covers NVIDIA GPUs; DLSS runs on nothing else anyway.
        if update.sdk == official::Sdk::Dlss
            && !tatu_upscalers::gpu::gpus().iter().any(|gpu| gpu.dlss)
        {
            return Err(
                "El DLSS oficial de NVIDIA es solo para placas NVIDIA con su driver".into(),
            );
        }
        refuse_anticheat(&g.root, accept_anticheat)?;

        let cache = pc::cache_dir();
        let placements = official::placements(&g.root, &update);
        let names: Vec<&str> = placements.iter().map(|(name, _)| *name).collect();
        // Another game may be fetching one of these very files.
        download.reserve(&names)?;
        // What `apply` would refuse anyway, before downloading up to 150 MB.
        refuse_while_running(&g)?;
        if let Some((rel, other)) = placements
            .iter()
            .find_map(|(_, dest)| Some((dest, patch::owner(&g.root, dest)?)))
        {
            return Err(UpscalerError::Overlap(rel.clone(), other).to_string());
        }
        // One progress bar for every file still to download.
        let total = official::missing_bytes(&cache, &names);
        let mut before = 0;
        let mut fetched = Vec::new();
        for (name, dest) in &placements {
            let mut last = 0;
            let path = official::ensure(
                &cache,
                name,
                |done, _| {
                    last = done;
                    let _ = app.emit(
                        "runtime_progress",
                        Progress {
                            done: before + done,
                            total,
                        },
                    );
                },
                &flag,
            )
            .map_err(|e| e.to_string())?;
            before += last;
            fetched.push((path, dest.clone()));
        }
        // Cancelled after the last byte (or with everything cached): still
        // before the game is touched.
        if flag.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(UpscalerError::Cancelled.to_string());
        }
        let plan = official::plan(&g.root, &update, fetched, g.running.clone());
        patch::apply(&g.root, &plan)
            .map(|m| PatchView::from(&m))
            .map_err(|e| e.to_string())
    })
    .await
}

fn file_name(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or_default()
}

/// The swap as a reversible patch: `new` in place of the game's `rel`.
fn swap_dlss(g: &Game, rel: &str, new: &Path) -> Result<PatchView, String> {
    let name = file_name(rel).to_lowercase();
    let new_name = new
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if !name.starts_with("nvngx_dlss") || new_name != name {
        return Err(format!("Elegí un archivo {name}"));
    }
    if !tatu_upscalers::is_dlss_dll(new) {
        return Err(format!("{} no es un DLL de DLSS de NVIDIA", new.display()));
    }
    let digest = format!("{:x}", Sha256::digest(rel.to_lowercase().as_bytes()));
    let plan = patch::Plan {
        id: format!("dlss-{}", &digest[..8]),
        sources: vec![patch::Source {
            from: new.to_path_buf(),
            rel: rel.to_string(),
            owned: false,
        }],
        runtime: Vec::new(),
        meta: serde_json::json!({
            "kind": "dlss",
            "file": rel,
            // `apply` validates `rel` before touching anything; reading
            // a version from it first is harmless.
            "from_version": tatu_upscalers::file_version(&g.root.join(rel)),
            "to_version": tatu_upscalers::file_version(new),
        }),
        running: g.running.clone(),
    };
    patch::apply(&g.root, &plan)
        .map(|m| PatchView::from(&m))
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct InstalledSettings {
    settings: Settings,
    /// The presets and FSR versions this OptiScaler documents.
    choices: settings::Choices,
}

/// The image settings the installed OptiScaler.ini has now, and what it
/// offers.
#[tauri::command]
pub async fn optiscaler_settings(
    state: State<'_, SharedState>,
    source: String,
    id: u64,
) -> Result<InstalledSettings, String> {
    let g = game(&state, &source, id).await?;
    blocking(move || {
        let target = installed_target(&g.root)?;
        let path = settings::ini_path(&g.root, &target);
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(InstalledSettings {
            settings: settings::read(&text),
            choices: settings::choices(&text),
        })
    })
    .await
}

/// Change the image settings of an installed OptiScaler — with the game
/// closed: OptiScaler rewrites its ini when its own menu saves.
#[tauri::command]
pub async fn optiscaler_save_settings(
    state: State<'_, SharedState>,
    busy: State<'_, PcBusy>,
    source: String,
    id: u64,
    settings: Settings,
) -> Result<(), String> {
    let g = game(&state, &source, id).await?;
    let _busy = busy.claim(&g.root)?;
    refuse_while_running(&g)?;
    blocking(move || {
        let target = installed_target(&g.root)?;
        settings::save(&g.root, &target, &settings).map_err(|e| e.to_string())
    })
    .await
}

/// The folder OptiScaler was installed into, from its patch record.
fn installed_target(root: &Path) -> Result<String, String> {
    let manifest = patch::load(root, optiscaler::PATCH_ID)
        .map_err(|e| e.to_string())?
        .ok_or("OptiScaler no está instalado en este juego")?;
    Ok(manifest.meta["target"]
        .as_str()
        .unwrap_or_default()
        .to_string())
}

/// Native file picker for a DLL: the user's own OptiScaler build, a newer
/// DLSS DLL. Async for the same reason as `pick_non_steam_folder`.
#[tauri::command]
pub async fn pc_pick_dll(app: AppHandle, start_in: Option<String>) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let mut builder = app.dialog().file().add_filter("DLL", &["dll"]);
    if let Some(dir) = start_in.filter(|d| !d.is_empty()) {
        builder = builder.set_directory(dir);
    }
    builder
        .blocking_pick_file()?
        .into_path()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

/// The folder OptiScaler goes into, when the detected one is wrong. `None`
/// goes back to detection. Compared after resolving links and `..`, so only a
/// folder really inside the game is accepted — however it was reached.
#[tauri::command]
pub async fn pc_set_target_dir(
    state: State<'_, SharedState>,
    source: String,
    id: u64,
    dir: Option<String>,
) -> Result<(), String> {
    let g = game(&state, &source, id).await?;
    let rel = match dir {
        None => None,
        Some(dir) => Some(
            blocking(move || {
                let root = std::fs::canonicalize(&g.root).map_err(|e| e.to_string())?;
                let picked = std::fs::canonicalize(&dir).map_err(|e| e.to_string())?;
                picked
                    .strip_prefix(&root)
                    .map(|rel| rel.to_string_lossy().replace('\\', "/"))
                    .map_err(|_| "La carpeta tiene que estar dentro de la del juego".to_string())
            })
            .await?,
        ),
    };
    let mut s = state.lock().map_err(|e| e.to_string())?;
    let key = pc::key(&source, id);
    match rel {
        None => s.pc.target_dirs.remove(&key),
        Some(rel) => s.pc.target_dirs.insert(key, rel),
    };
    s.save();
    Ok(())
}

/// Bring the Steam launch options in line with what's installed: add the
/// override OptiScaler needs, or put back what was there once it's gone.
/// `close_steam`: close it first (it rewrites the file on exit otherwise).
#[tauri::command]
pub async fn pc_launch_sync(
    state: State<'_, SharedState>,
    source: String,
    id: u64,
    close_steam: bool,
) -> Result<LaunchReport, String> {
    let g = game(&state, &source, id).await?;
    if close_steam {
        close_steam_for(&g).await?;
    }
    Ok(launch_sync(&state, &g).await)
}

/// Turn one of GE-Proton's upgrade switches on or off in a Steam game's
/// launch options.
#[tauri::command]
pub async fn pc_set_proton_upgrade(
    state: State<'_, SharedState>,
    source: String,
    id: u64,
    key: String,
    on: bool,
    close_steam: bool,
) -> Result<LaunchReport, String> {
    if source != "steam" || !PROTON_UPGRADES.contains(&key.as_str()) {
        return Err("Opción no disponible para este juego".into());
    }
    let g = game(&state, &source, id).await?;
    if close_steam {
        close_steam_for(&g).await?;
    }
    set_env_blocking(g, key, on).await
}

#[cfg(unix)]
async fn set_env_blocking(g: Game, key: String, on: bool) -> Result<LaunchReport, String> {
    let app = g.steam_app.clone().unwrap_or_default();
    blocking(move || {
        Ok(
            match crate::steam::current_account().and_then(|account| {
                crate::steam::update_app_launch_options(account, &app, |current| {
                    crate::steam::set_env(current, &key, on.then_some("1"))
                })
            }) {
                Ok(_) => LaunchReport::new("applied", key),
                Err(e) => launch_error(&e, &key),
            },
        )
    })
    .await
}

#[cfg(not(unix))]
async fn set_env_blocking(_: Game, _: String, _: bool) -> Result<LaunchReport, String> {
    Err("Solo en Linux (GE-Proton)".into())
}

/// Close Steam so its launch options can be edited — never under a running
/// game.
async fn close_steam_for(g: &Game) -> Result<(), String> {
    refuse_while_running(g).map_err(|_| "Cerrá el juego antes de cerrar Steam".to_string())?;
    #[cfg(unix)]
    {
        blocking(|| {
            crate::steam::stop_steam_for_config_edit()
                .map_err(|_| "No pude cerrar Steam: cerralo a mano y probá de nuevo".to_string())
        })
        .await
    }
    #[cfg(not(unix))]
    {
        Ok(())
    }
}

/// What a launch-option sync wants recorded about Tatu's own changes, by
/// [`pc::launch_key`].
#[cfg_attr(not(unix), allow(dead_code))]
enum Record {
    Remove(String),
    Set(String, LaunchChange),
}

/// [`sync_launch`] off the main thread, then its records applied under a
/// short lock — the state is re-read, never taken from an older snapshot.
async fn launch_sync(shared: &SharedState, g: &Game) -> LaunchReport {
    let recorded = g
        .steam_app
        .as_ref()
        .and_then(|app| {
            let s = shared.lock().ok()?;
            Some(pc::launch_changes_for(&s.pc, app))
        })
        .unwrap_or_default();
    let job = g.clone();
    let (report, records) =
        match tokio::task::spawn_blocking(move || sync_launch(&job, recorded)).await {
            Ok(result) => result,
            Err(e) => return LaunchReport::new("error", e.to_string()),
        };
    if !records.is_empty()
        && let Ok(mut s) = shared.lock()
    {
        for record in records {
            match record {
                Record::Remove(key) => {
                    s.pc.launch_changes.remove(&key);
                }
                Record::Set(key, change) => {
                    s.pc.launch_changes.insert(key, change);
                }
            }
        }
        s.save();
    }
    report
}

#[cfg(not(unix))]
fn sync_launch(_: &Game, _: Vec<(String, LaunchChange)>) -> (LaunchReport, Vec<Record>) {
    (LaunchReport::new("not_needed", ""), Vec::new())
}

/// The one place launch options change for OptiScaler. Idempotent: called
/// after install, after revert, and again from the tab once Steam is closed.
/// Every edit reads, changes and writes as one step
/// (`update_app_launch_options`). The override goes into the active
/// account's options; an earlier change is undone on the account it was
/// made on, whichever is active now.
#[cfg(unix)]
fn sync_launch(g: &Game, recorded: Vec<(String, LaunchChange)>) -> (LaunchReport, Vec<Record>) {
    use crate::steam::{AddOverride, LaunchOptError};

    let proxy = patch::load(&g.root, optiscaler::PATCH_ID)
        .ok()
        .flatten()
        .and_then(|m| m.meta["proxy"].as_str().map(str::to_string));
    let dll = proxy
        .filter(|p| !PROTON_NATIVE.contains(&p.as_str()))
        .map(|p| p.trim_end_matches(".dll").to_string());

    let Some(app) = &g.steam_app else {
        let report = match dll {
            Some(dll) => LaunchReport::new(
                "manual",
                format!(
                    "Agregá WINEDLLOVERRIDES=\"{dll}=n,b\" a las variables de entorno del lanzador del juego"
                ),
            ),
            None => LaunchReport::new("not_needed", ""),
        };
        return (report, Vec::new());
    };

    // Undo Tatu's earlier changes when OptiScaler is gone or now uses another
    // name: exactly what the user had if nothing else changed since, else
    // only the entry Tatu added.
    let mut report = LaunchReport::new("not_needed", "");
    let mut records = Vec::new();
    for (key, rec) in recorded
        .iter()
        .filter(|(_, rec)| dll.as_deref() != Some(rec.dll.as_str()))
    {
        let entry = format!("{}=n,b", rec.dll);
        // `0`: recorded before accounts were — made on the active one.
        let account = match rec.account {
            0 => match crate::steam::current_account() {
                Ok(account) => account,
                Err(e) => return (launch_error(&e, &entry), records),
            },
            account => account,
        };
        let undone = crate::steam::update_app_launch_options(account, app, |current| {
            Some(if current == rec.after {
                rec.before.clone()
            } else {
                crate::steam::remove_override(current, &rec.dll)
                    .unwrap_or_else(|| current.to_string())
            })
        });
        match undone {
            Ok(_) => report = LaunchReport::new("removed", entry),
            // That account's Steam data is gone, and the change with it.
            Err(LaunchOptError::ConfigNotFound(_)) => {}
            Err(e) => return (launch_error(&e, &entry), records),
        }
        records.push(Record::Remove(key.clone()));
    }

    let Some(dll) = dll else {
        return (report, records);
    };
    let entry = format!("{dll}=n,b");
    let account = match crate::steam::current_account() {
        Ok(account) => account,
        Err(e) => return (launch_error(&e, &entry), records),
    };
    let mut found = None;
    let added = crate::steam::update_app_launch_options(account, app, |current| {
        match crate::steam::add_override(current, &dll) {
            AddOverride::Added(after) => Some(after),
            other => {
                found = Some(other);
                None
            }
        }
    });
    let report = match (added, found) {
        (Ok(Some((before, after))), _) => {
            records.push(Record::Set(
                pc::launch_key(account, app),
                LaunchChange {
                    account,
                    dll: dll.clone(),
                    before,
                    after,
                },
            ));
            LaunchReport::new("applied", entry)
        }
        (Ok(None), Some(AddOverride::Conflict(mode))) => LaunchReport::new(
            "conflict",
            format!(
                "Tus opciones de lanzamiento ya tienen {dll}={mode}: OptiScaler no va a cargar hasta que lo cambies a {entry}"
            ),
        ),
        (Ok(None), _) => LaunchReport::new("already", entry),
        (Err(e), _) => launch_error(&e, &entry),
    };
    (report, records)
}

/// Steam's config errors, for the (Spanish) UI. Steam running is its own
/// status: the tab offers to close it.
#[cfg(unix)]
fn launch_error(e: &crate::steam::LaunchOptError, what: &str) -> LaunchReport {
    match e {
        crate::steam::LaunchOptError::SteamRunning => LaunchReport::new("steam_running", what),
        _ => LaunchReport::new("error", launch_error_text(e)),
    }
}

#[cfg(unix)]
fn launch_error_text(e: &crate::steam::LaunchOptError) -> String {
    use crate::steam::LaunchOptError as E;
    match e {
        E::SteamRunning => "Steam está abierto: cerralo primero".to_string(),
        E::SteamIdNotFound => "No encontré la cuenta de Steam activa".to_string(),
        E::ConfigNotFound(path) => {
            format!("No encontré la configuración de Steam ({})", path.display())
        }
        E::Io { path, source } => format!("{}: {source}", path.display()),
        E::Vdf(e) => format!("No pude leer la configuración de Steam: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downloads_run_side_by_side_but_never_on_the_same_file() {
        let downloads = Arc::new(Downloads::default());
        let mut first = downloads.claim("t1").unwrap();
        first.reserve(&["nvngx_dlss.dll"]).unwrap();
        // Another game, another file: fine. The same file: refused.
        let mut second = downloads.claim("t2").unwrap();
        second.reserve(&["libxess.dll"]).unwrap();
        assert!(
            downloads
                .claim("t3")
                .unwrap()
                .reserve(&["nvngx_dlss.dll"])
                .is_err()
        );

        // Cancel reaches the one that asked for it, and no other.
        downloads.cancel("t2");
        assert!(!first.flag.load(std::sync::atomic::Ordering::Relaxed));
        assert!(second.flag.load(std::sync::atomic::Ordering::Relaxed));
        downloads.cancel("gone");

        // Finished: its files and token are free again.
        drop(first);
        let mut third = downloads.claim("t4").unwrap();
        third.reserve(&["nvngx_dlss.dll"]).unwrap();
        drop(second);
        drop(third);
        let running = downloads.0.lock().unwrap();
        assert!(running.files.is_empty() && running.by_token.is_empty());
    }
}
