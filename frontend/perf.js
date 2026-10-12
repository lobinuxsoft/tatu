import { invoke, listen, getCurrentWindow } from "./js/tauri.js";
import { esc } from "./js/utils.js";
import { installExternalLinks } from "./js/links.js";
import { initTheme } from "./js/themes.js";
import { detailTabsShell, installDetailTabSwitcher } from "./js/panel/detail_template.js";
import { mountLive } from "./js/perf/live.js";
import { mountSessions } from "./js/perf/sessions.js";

// The performance window (GameNative tab → "Medir rendimiento"): one game at a
// time, retargeted in place like the detail window — except while
// recording, when the backend keeps it on the game being measured.

initTheme();
installExternalLinks();

let views = [];

function show(game) {
  const content = document.getElementById("perfContent");
  if (!game) {
    content.innerHTML = `<div class="loading">No hay un juego elegido.</div>`;
    return;
  }
  getCurrentWindow().setTitle(`${game.name} — Rendimiento`).catch(() => {});
  content.innerHTML =
    `<div class="perf-header"><div class="detail-title">${esc(game.name)}</div>` +
    `<div class="modal-hint">Rendimiento del juego en GameNative (Android), medido por adb desde esta PC.</div></div>` +
    detailTabsShell([
      { key: "vivo", label: "En vivo", initialHtml: `<div id="perfLive"></div>` },
      { key: "sesiones", label: "Sesiones", initialHtml: `<div id="perfSessions"></div>` },
    ]);
  installDetailTabSwitcher();

  const sessions = mountSessions(document.getElementById("perfSessions"), game);
  const live = mountLive(document.getElementById("perfLive"), game, { onSaved: () => sessions.refresh() });
  views = [live, sessions];

  // Charts are drawn at their card's real width, and one hidden in the other
  // tab has none — so switching tabs redraws.
  document.querySelector(".detail-tabs").addEventListener("click", e => {
    const tab = e.target.closest(".detail-tab");
    if (!tab) return;
    if (tab.dataset.dp === "sesiones") sessions.refresh();
    else live.redraw();
  });
}

let resizeTimer = null;
window.addEventListener("resize", () => {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(() => views.forEach(v => v.redraw()), 150);
});

listen("perf-target-changed", e => show(e.payload));
invoke("perf_target")
  .then(show)
  .catch(e => {
    document.getElementById("perfContent").innerHTML = `<div class="loading">${esc(String(e))}</div>`;
  });
