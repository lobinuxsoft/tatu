use tauri::{Emitter, Manager, State};

use crate::SharedState;
use crate::drm;

/// DRM info cache TTL: 30 days. DRM rarely changes after release.
const DRM_CACHE_TTL_SECS: u64 = 60 * 60 * 24 * 30;

/// Throttle between PCGamingWiki / Steam Store requests during bulk DRM sync.
const DRM_BULK_DELAY_MS: u64 = 1000;

/// A cached DrmInfo is considered stale if its TTL expired, if it is a
/// pre-feature record that predates the preservability classifier (detected
/// by the hint being empty while status was successfully classified), or if
/// it was classified as Unknown from Steam Store data alone (`source ==
/// "steam"`, meaning PCGamingWiki contributed nothing) — the common shape
/// every entry took while PCGW's August 2026 migration blocked anonymous
/// `cargoquery` outright (see `login_pcgw`). Worth a retry now that auth
/// exists, regardless of how "fresh" the cache thinks that non-answer is.
fn drm_cache_is_stale(cached: &drm::DrmInfo, now: u64) -> bool {
    if now.saturating_sub(cached.fetched_at) >= DRM_CACHE_TTL_SECS {
        return true;
    }
    if cached.preservability_hint.is_empty() && !matches!(cached.status, drm::DrmStatus::Unknown) {
        return true;
    }
    if matches!(cached.status, drm::DrmStatus::Unknown) && cached.source == "steam" {
        return true;
    }
    false
}

#[tauri::command]
pub fn fetch_all_drm(app: tauri::AppHandle, state: State<'_, SharedState>) -> Result<(), String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let pending: Vec<u64> = s
        .games
        .iter()
        .filter(|g| {
            s.drm_cache
                .get(&g.id)
                .map(|cached| drm_cache_is_stale(cached, now))
                .unwrap_or(true)
        })
        .map(|g| g.id)
        .collect();
    // Logged in once for the whole bulk run — see `login_pcgw`'s own doc
    // comment for why per-game login would waste PCGW's rate-limit budget.
    let pcgw_agent = drm::login_pcgw(&s.pcgw_username, &s.pcgw_bot_password);
    drop(s);

    let app_clone = app.clone();
    std::thread::spawn(move || {
        let total = pending.len();
        for (i, app_id) in pending.iter().enumerate() {
            let info = match drm::fetch_drm_info(*app_id, pcgw_agent.as_ref()) {
                Ok(v) => v,
                Err(_) => {
                    let _ = app_clone.emit(
                        "drm_progress",
                        serde_json::json!({ "current": i + 1, "total": total }),
                    );
                    std::thread::sleep(std::time::Duration::from_millis(DRM_BULK_DELAY_MS));
                    continue;
                }
            };
            let state: tauri::State<'_, SharedState> = app_clone.state();
            if let Ok(mut s) = state.lock() {
                s.drm_cache.insert(*app_id, info.clone());
                s.save();
            }
            let _ = app_clone.emit(
                "drm_progress",
                serde_json::json!({ "current": i + 1, "total": total, "app_id": app_id, "info": info }),
            );
            std::thread::sleep(std::time::Duration::from_millis(DRM_BULK_DELAY_MS));
        }
        let _ = app_clone.emit("drm_done", serde_json::json!({ "total": total }));
    });

    Ok(())
}

#[tauri::command]
pub async fn get_game_drm(
    app_id: u64,
    state: State<'_, SharedState>,
) -> Result<drm::DrmInfo, String> {
    let (pcgw_username, pcgw_bot_password) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        if let Some(cached) = s.drm_cache.get(&app_id) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            if !drm_cache_is_stale(cached, now) {
                return Ok(cached.clone());
            }
        }
        (s.pcgw_username.clone(), s.pcgw_bot_password.clone())
    };

    let info = tokio::task::spawn_blocking(move || {
        let pcgw_agent = drm::login_pcgw(&pcgw_username, &pcgw_bot_password);
        drm::fetch_drm_info(app_id, pcgw_agent.as_ref())
    })
    .await
    .map_err(|e| e.to_string())??;

    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.drm_cache.insert(app_id, info.clone());
    s.save();
    Ok(info)
}
