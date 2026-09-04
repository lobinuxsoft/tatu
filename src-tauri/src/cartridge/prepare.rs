use std::path::Path;

use crate::drm::{self, DrmInfo, Preservability};

use super::goldberg::{inject_goldberg, is_still_injected};
use super::install::sync_marker_with_installed_apps;
use super::marker::{AppSource, CartridgeApp, add_app, list_apps};

/// Per-app outcome, for the UI to report progress and results as it goes.
/// Carries the full `DrmInfo`, not just `Preservability` — the caller also
/// needs it to refresh the main library's own DRM cache, not only the
/// cartridge marker, so "Desconocido" in the Steam tab gets fixed too.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PrepareDrmResult {
    pub app_id: u64,
    pub name: String,
    /// `None` only when the DRM re-fetch itself failed (see `error`) — the
    /// old classification is left untouched in that case, nothing to report.
    pub drm_info: Option<DrmInfo>,
    pub goldberg_injected: bool,
    pub error: Option<String>,
}

/// Re-classifies every app already recorded on this cartridge — not just
/// whatever was known at install time — and injects Goldberg for any that
/// newly resolve to `Easy` and aren't standalone yet. Run as part of
/// "Preparar launcher" so it's automatic and covers every installed game,
/// not a per-game manual step: requested live (2026-08-28) after Hellpoint
/// installed while its own DRM was still "Desconocido", the local-file
/// probe (#238) later confirmed it's Goldberg-compatible, and nothing had
/// ever revisited it after install to act on that.
///
/// Every `Preservability` kind maps to a defined action (or deliberately
/// none), so this never has to guess:
/// - `Easy`, not yet standalone → inject Goldberg.
/// - `Easy`, already standalone → re-inject only if the active DLL drifted
///   away from Goldberg's own (see `is_still_injected`), otherwise nothing
///   to do.
/// - `Trivial` / `Removed` → nothing needed, the Steam copy already has no
///   active DRM to work around.
/// - `Alternative` (GOG) → nothing to do to the Steam copy itself — the
///   marker is still refreshed so the UI reflects it accurately.
/// - `Hard` / `Unknown` → left untouched, the same refusal `inject_goldberg`
///   already enforces on its own: no safe automatic action exists for
///   either.
///
/// GOG apps (`AppSource::Gog`, #243) are skipped entirely before any of the
/// above: their `app_id` is a GOG product id, not a Steam appid, so
/// `drm::fetch_drm_info` would be querying Steam DRM sources with the wrong
/// kind of number. Non-Steam (non-GOG) entries still aren't covered — no
/// cartridge tracking for those yet at all (#236).
pub fn refresh_drm_and_inject(
    mount_point: &Path,
    template_x86: &Path,
    template_x64: &Path,
    pcgw_agent: Option<&ureq::Agent>,
    steam_id: &str,
) -> Result<Vec<PrepareDrmResult>, String> {
    // Reconciles the marker against Steam's own manifests first (#244) — a
    // game installed directly through Steam, rather than this app's own
    // per-game flow, never touched the marker at all otherwise, and would
    // silently miss out on DRM classification/Goldberg below.
    sync_marker_with_installed_apps(mount_point)?;
    let apps = list_apps(mount_point)?;
    let mut results = Vec::with_capacity(apps.len());

    for app in apps {
        // GOG apps are never Steam entries and have no Steam DRM to
        // reclassify — `Preservability::Alternative` is already their
        // final, correct answer from the moment they're installed. Calling
        // `fetch_drm_info` with a GOG product id would query Steam's own
        // DRM sources by a number that isn't a Steam appid at all.
        if app.source == AppSource::Gog {
            results.push(PrepareDrmResult {
                app_id: app.app_id,
                name: app.name,
                drm_info: None,
                goldberg_injected: false,
                error: None,
            });
            continue;
        }
        let info = match drm::fetch_drm_info(app.app_id, pcgw_agent) {
            Ok(info) => info,
            Err(e) => {
                results.push(PrepareDrmResult {
                    app_id: app.app_id,
                    name: app.name,
                    drm_info: None,
                    goldberg_injected: false,
                    error: Some(e),
                });
                continue;
            }
        };
        let fresh = info.preservability.clone();

        // Persist the refreshed classification regardless of what happens
        // next, so "Gestionar cartucho" always shows the latest known
        // status even for kinds that need no file action below. Best
        // effort: a write failure here doesn't need to abort the batch,
        // the same underlying disk problem would surface loudly through
        // inject_goldberg's own error for this app anyway.
        let _ = add_app(
            mount_point,
            CartridgeApp {
                app_id: app.app_id,
                name: app.name.clone(),
                source: app.source,
                preservability: fresh.clone(),
                standalone: app.standalone,
                exe_path: app.exe_path,
            },
        );

        let mut goldberg_injected = false;
        let mut error = None;
        let drifted = fresh == Preservability::Easy
            && app.standalone
            && !is_still_injected(mount_point, app.app_id, template_x86, template_x64);
        if fresh == Preservability::Easy && (!app.standalone || drifted) {
            match inject_goldberg(
                mount_point,
                app.app_id,
                fresh,
                template_x86,
                template_x64,
                steam_id,
            ) {
                Ok(()) => goldberg_injected = true,
                Err(e) => error = Some(e),
            }
        }

        results.push(PrepareDrmResult {
            app_id: app.app_id,
            name: app.name,
            drm_info: Some(info),
            goldberg_injected,
            error,
        });
    }

    Ok(results)
}
