import { invoke, getCurrentWindow } from "../tauri.js";
import { state } from "../state.js";
import { esc, formatBytes } from "../utils.js";
import { openEgsCartridgeModal } from "../modals/egs_cartridge.js";
import { artworkTabHtml, installArtworkTabHandlers } from "./artwork_tab.js";
import { loadEgsDrm } from "./loaders.js";
import {
  detailHeaderShell,
  headerImg,
  infoRow,
  detailTabsShell,
  installDetailTabSwitcher,
} from "./detail_template.js";

// Same shared window template Steam/GOG's detail views use (#243). Just an
// "Info" tab plus "Arte" — no Logros/Cromos/Cheats (real Steam APIs GOG/EGS
// have no equivalent of). Description/screenshots stay minimal until #347
// resolves the storefront slug problem.
export function renderEgsDetail(game) {
  // Same race-guard `panel/detail.js::renderDetail` sets for Steam —
  // loadEgsDrm checks this before touching the DOM, in case the user
  // navigates to a different game while the PCGW request is in flight.
  state.panelGameId = game.id;

  let infoHtml = `<div class="detail-info-row"><button class="cartridge-btn" id="egsCartridgeBtn">💾 Instalar en cartucho</button></div>`;
  if (game.developers && game.developers.length) {
    infoHtml += infoRow("Developer", esc(game.developers.join(", ")));
  }
  infoHtml += infoRow("Plataforma", "Epic Games Store");
  infoHtml += infoRow("App name", esc(game.app_name));
  infoHtml += infoRow("Peso de descarga", "Consultando...", "dpEgsSize");
  infoHtml += infoRow("DRM", `<span style="color:#6e7681">Consultando...</span>`, "dpEgsDrm");
  if (game.description) {
    infoHtml +=
      `<details class="detail-desc-toggle" open><summary>Descripción</summary>` +
      `<div class="detail-desc" style="white-space:pre-wrap;">${esc(game.description)}</div></details>`;
  }

  document.getElementById("detailContent").innerHTML =
    detailHeaderShell(game.title, `<span>Epic Games Store</span>`, headerImg(game.background_url)) +
    detailTabsShell([
      { key: "info", label: "Info", initialHtml: infoHtml },
      { key: "arte", label: "Arte", initialHtml: artworkTabHtml() },
    ]);
  installDetailTabSwitcher();
  document.getElementById("egsCartridgeBtn").onclick = () => openEgsCartridgeModal(game.id);
  installArtworkTabHandlers(game.id, game.title);

  getCurrentWindow().setTitle(game.title + " — Tatu").catch(() => {});

  loadEgsDrm(game.id, game.title);

  // Fetches and parses the real manifest (no chunk bytes) — cheap enough to
  // run every time the detail window opens, same reasoning gog_detail.js's
  // own size row uses.
  invoke("egs_get_download_size", { appId: game.id })
    .then(info => {
      const el = document.getElementById("dpEgsSize");
      if (el) el.querySelector(".detail-info-value").textContent = formatBytes(info.total_size);
    })
    .catch(e => {
      const el = document.getElementById("dpEgsSize");
      if (el) {
        const value = el.querySelector(".detail-info-value");
        value.textContent = String(e);
        value.style.color = "#484f58";
      }
    });
}
