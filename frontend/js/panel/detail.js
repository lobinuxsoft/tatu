import { state } from "../state.js";
import { getCurrentWindow } from "../tauri.js";
import { esc } from "../utils.js";
import { startLoading, clearLoadingTasks } from "../loading.js";
import { loadGameDetails, loadAchievements, loadCards } from "./loaders.js";
import { loadCheats } from "./cheats_view.js";

// Renders the detail view into `#detailContent`. Since #187 that container
// lives in its own OS window (detail.html), not in a modal inside the main
// one — so there is no overlay to raise and no close button to wire: the
// window title bar owns both.
export function renderDetail(gameId) {
  const g = state.G.find(x => x.id === gameId);
  if (!g) {
    document.getElementById("detailContent").innerHTML =
      `<div class="loading">No encontré ese juego en la biblioteca.</div>`;
    return;
  }
  state.panelOpen = true;
  state.panelGameId = gameId;

  const h = g.hours > 0 ? g.hours + "h" : "—";

  document.getElementById("detailContent").innerHTML =
    `<div class="detail-header">` +
      `<div id="dpHeaderImg"></div>` +
      `<div class="detail-header-info">` +
        `<div class="detail-title">${esc(g.name)}</div>` +
        `<div class="detail-meta"><span>${h} jugadas</span><span id="dpMetaExtra"></span></div>` +
      `</div>` +
    `</div>` +
    `<div class="detail-tabs">` +
      `<div class="detail-tab active" data-dp="info">Info</div>` +
      `<div class="detail-tab" data-dp="logros">Logros</div>` +
      (g.has_cards ? `<div class="detail-tab" data-dp="cromos">Cromos</div>` : ``) +
      (state.cheatsSupported ? `<div class="detail-tab" data-dp="cheats">Cheats</div>` : ``) +
    `</div>` +
    `<div class="detail-tab-panel active" id="dpInfo"><div class="loading"><div class="spinner"></div><br>Cargando info...</div></div>` +
    `<div class="detail-tab-panel" id="dpLogros"><div class="loading"><div class="spinner"></div><br>Cargando logros...</div></div>` +
    (g.has_cards ? `<div class="detail-tab-panel" id="dpCromos"><div class="loading"><div class="spinner"></div><br>Cargando inventario de Steam (puede tardar unos segundos)...</div></div>` : ``) +
    (state.cheatsSupported ? `<div class="detail-tab-panel" id="dpCheats"><div class="loading"><div class="spinner"></div><br>Cargando cheats...</div></div>` : ``);

  document.querySelector(".detail-tabs").onclick = e => {
    const tab = e.target.closest(".detail-tab");
    if (!tab) return;
    document.querySelectorAll(".detail-tab").forEach(t => t.classList.remove("active"));
    document.querySelectorAll(".detail-tab-panel").forEach(p => p.classList.remove("active"));
    tab.classList.add("active");
    document.getElementById("dp" + tab.dataset.dp.charAt(0).toUpperCase() + tab.dataset.dp.slice(1)).classList.add("active");
  };

  // Naming the OS window after the game is the point of having a window:
  // on a second monitor the task bar entry has to say which game it is.
  // document.title does not reach the native title bar, so ask Tauri.
  getCurrentWindow().setTitle(g.name + " — Tatu").catch(() => {});

  clearLoadingTasks();
  startLoading("info");
  startLoading("logros");
  loadGameDetails(gameId);
  loadAchievements(gameId);
  if (g.has_cards) { startLoading("cromos"); loadCards(gameId); }
  if (state.cheatsSupported) loadCheats(gameId);
}
