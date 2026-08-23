import { invoke, listen } from "./js/tauri.js";
import { state } from "./js/state.js";
import { renderDetail } from "./js/panel/detail.js";
import { installExternalLinks } from "./js/links.js";
import { installLightbox } from "./js/panel/lightbox.js";
import { initTheme } from "./js/themes.js";

initTheme();
installExternalLinks();
installLightbox();

// This window has no library of its own: the panel modules read titles, hours
// and completion out of `state`, so it loads the same snapshot the main window
// does before rendering anything.
async function loadLibrary() {
  const data = await invoke("get_state");
  state.G = data.games || [];
  state.NS = data.non_steam || [];
  state.completed = new Set(data.completed || []);
  state.completedNS = new Set(data.completed_nonsteam || []);
  state.achProgress = data.ach_progress || {};
  state.hltbCache = data.hltb_cache || {};
  state.drmCache = data.drm_cache || {};
  state.sizeCache = data.size_cache || {};
}

async function show(gameId) {
  if (gameId === null || gameId === undefined) return;
  renderDetail(gameId);
}

async function boot() {
  try {
    state.cheatsSupported = await invoke("cheats_supported");
  } catch (_) {
    state.cheatsSupported = false;
  }

  try {
    await loadLibrary();
  } catch (e) {
    document.getElementById("detailContent").innerHTML =
      `<div class="loading" style="color:var(--danger)">No pude leer la biblioteca: ${e}</div>`;
    return;
  }

  await show(await invoke("detail_target"));
}

// Clicking another game reuses this window rather than opening a second one,
// so the backend retargets it and we re-render in place.
listen("detail-target-changed", e => {
  loadLibrary().catch(() => {}).finally(() => show(e.payload));
});

boot();
