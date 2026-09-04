import { state } from "../state.js";
import { getCurrentWindow } from "../tauri.js";
import { startLoading, clearLoadingTasks } from "../loading.js";
import { loadGameDetails, loadAchievements, loadCards } from "./loaders.js";
import { loadCheats } from "./cheats_view.js";
import { detailHeaderShell, detailTabsShell, installDetailTabSwitcher, loadingPlaceholder } from "./detail_template.js";

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

  const tabs = [
    { key: "info", label: "Info", initialHtml: loadingPlaceholder("Cargando info...") },
    { key: "logros", label: "Logros", initialHtml: loadingPlaceholder("Cargando logros...") },
  ];
  if (g.has_cards) {
    tabs.push({ key: "cromos", label: "Cromos", initialHtml: loadingPlaceholder("Cargando inventario de Steam (puede tardar unos segundos)...") });
  }
  if (state.cheatsSupported) {
    tabs.push({ key: "cheats", label: "Cheats", initialHtml: loadingPlaceholder("Cargando cheats...") });
  }

  document.getElementById("detailContent").innerHTML =
    detailHeaderShell(g.name, `<span>${h} jugadas</span><span id="dpMetaExtra"></span>`) +
    detailTabsShell(tabs);
  installDetailTabSwitcher();

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
