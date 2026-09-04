import { invoke, listen } from "./js/tauri.js";
import { state } from "./js/state.js";
import { renderDetail } from "./js/panel/detail.js";
import { renderGogDetail } from "./js/panel/gog_detail.js";
import { installExternalLinks } from "./js/links.js";
import { installLightbox } from "./js/panel/lightbox.js";
import { installCardTilt } from "./js/panel/card_tilt.js";
import { initTheme } from "./js/themes.js";
import { closeCartridgeModal } from "./js/modals/cartridge.js";
import { closeGogCartridgeModal } from "./js/modals/gog_cartridge.js";

initTheme();
installExternalLinks();
installLightbox();
installCardTilt();

// Steam's install (trigger-and-poll through Steam itself) and GOG's
// (Tatu downloads the bytes, #243) are different enough flows to need
// their own modal module — this window shows one game from one source at
// a time, so which close function applies just follows `show()`'s target.
let activeCartridgeClose = closeCartridgeModal;
document.getElementById("cartridgeClose").addEventListener("click", () => activeCartridgeClose());
document.getElementById("cartridgeOverlay").addEventListener("click", e => {
  if (e.target.id === "cartridgeOverlay") activeCartridgeClose();
});

// This window has no library of its own: the panel modules read the target
// game (and its cached DRM info, for the cartridge modal) out of `state`.
// Used to load the WHOLE main-window snapshot (get_state) just for that —
// every game plus the full DRM/HLTB/achievement/size caches, 2.8MB+ for a
// 540-game library, shipped over IPC before a single pixel of this window
// drew anything. That transfer time was the entire "feels hung" symptom
// reported live (2026-08-28): a blank window with no spinner while it
// happened. get_game_context asks only for the one game this window
// actually needs — everything else (info, achievements, cards, cheats) was
// already its own targeted per-game fetch inside loaders.js/cheats_view.js,
// never read from this bulk snapshot at all.
async function loadSteamGameContext(gameId) {
  const ctx = await invoke("get_game_context", { appId: gameId });
  state.G = ctx.game ? [ctx.game] : [];
  state.NS = ctx.non_steam_game ? [ctx.non_steam_game] : [];
  state.completed = new Set();
  state.completedNS = new Set();
  state.achProgress = {};
  state.hltbCache = {};
  state.drmCache = ctx.drm_info ? { [gameId]: ctx.drm_info } : {};
  state.sizeCache = {};
}

// A GOG product id and a Steam appid share no namespace (#243) — `target`
// always carries which collection to look the id up in, so this never
// risks matching an unrelated Steam game that happens to share the number.
async function show(target) {
  if (!target || target.app_id === null || target.app_id === undefined) return;
  if (target.source === "gog") {
    activeCartridgeClose = closeGogCartridgeModal;
    const game = await invoke("get_gog_game_context", { appId: target.app_id });
    if (!game) {
      document.getElementById("detailContent").innerHTML =
        `<div class="loading">No encontré ese juego en tu biblioteca de GOG.</div>`;
      return;
    }
    state.GOG = [game];
    renderGogDetail(game);
    return;
  }
  activeCartridgeClose = closeCartridgeModal;
  await loadSteamGameContext(target.app_id);
  renderDetail(target.app_id);
}

async function boot() {
  try {
    state.cheatsSupported = await invoke("cheats_supported");
  } catch (_) {
    state.cheatsSupported = false;
  }

  const target = await invoke("detail_target");
  try {
    await show(target);
  } catch (e) {
    document.getElementById("detailContent").innerHTML =
      `<div class="loading" style="color:var(--danger)">No pude leer el juego: ${e}</div>`;
  }
}

// Clicking another game reuses this window rather than opening a second one,
// so the backend retargets it and we re-render in place.
listen("detail-target-changed", e => {
  show(e.payload).catch(() => {});
});

boot();
