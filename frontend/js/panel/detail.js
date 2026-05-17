import { state } from "../state.js";
import { esc } from "../utils.js";
import { startLoading, clearLoadingTasks } from "../loading.js";
import { loadGameDetails, loadAchievements, loadCards } from "./loaders.js";
import { loadCheats } from "./cheats_view.js";

export function openDetailPanel(gameId) {
  const g = state.G.find(x => x.id === gameId);
  if (!g) return;
  state.panelOpen = true;
  state.panelGameId = gameId;

  const h = g.hours > 0 ? g.hours + "h" : "\u2014";

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
      `<div class="detail-tab" data-dp="cheats">Cheats</div>` +
    `</div>` +
    `<div class="detail-tab-panel active" id="dpInfo"><div class="loading"><div class="spinner"></div><br>Cargando info...</div></div>` +
    `<div class="detail-tab-panel" id="dpLogros"><div class="loading"><div class="spinner"></div><br>Cargando logros...</div></div>` +
    (g.has_cards ? `<div class="detail-tab-panel" id="dpCromos"><div class="loading"><div class="spinner"></div><br>Cargando inventario de Steam (puede tardar unos segundos)...</div></div>` : ``) +
    `<div class="detail-tab-panel" id="dpCheats"><div class="loading"><div class="spinner"></div><br>Cargando cheats...</div></div>`;

  document.getElementById("detailOverlay").classList.add("open");
  document.getElementById("detailPanel").classList.add("open");

  document.querySelector(".detail-tabs").onclick = e => {
    const tab = e.target.closest(".detail-tab");
    if (!tab) return;
    document.querySelectorAll(".detail-tab").forEach(t => t.classList.remove("active"));
    document.querySelectorAll(".detail-tab-panel").forEach(p => p.classList.remove("active"));
    tab.classList.add("active");
    document.getElementById("dp" + tab.dataset.dp.charAt(0).toUpperCase() + tab.dataset.dp.slice(1)).classList.add("active");
  };

  clearLoadingTasks();
  startLoading("info");
  startLoading("logros");
  loadGameDetails(gameId);
  loadAchievements(gameId);
  if (g.has_cards) { startLoading("cromos"); loadCards(gameId); }
  loadCheats(gameId);
}

export function closeDetailPanel() {
  document.getElementById("detailOverlay").classList.remove("open");
  document.getElementById("detailPanel").classList.remove("open");
  clearLoadingTasks();
  state.panelOpen = false;
  state.panelGameId = null;
}
