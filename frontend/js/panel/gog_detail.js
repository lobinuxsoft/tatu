import { invoke, getCurrentWindow } from "../tauri.js";
import { esc, formatBytes } from "../utils.js";
import { openGogCartridgeModal } from "../modals/gog_cartridge.js";
import {
  detailHeaderShell,
  headerImg,
  infoRow,
  tagsRow,
  cardGridItem,
  cardGrid,
  detailTabsShell,
  installDetailTabSwitcher,
  loadingPlaceholder,
} from "./detail_template.js";

// GOG's detail view is the SAME window template Steam's is (#243) —
// header, tab bar/switcher, info-row/tags/card-grid markup, all from
// js/panel/detail_template.js, not a second hand-written copy. It renders
// with just one tab ("info") instead of Steam's up-to-four: Logros/Cromos/
// Cheats are real Steam APIs (achievements, trading cards, Cheat Engine
// tables keyed to a Steam appid) GOG has no equivalent of — an empty tab
// pretending otherwise would be worse than not having it.
export function renderGogDetail(game) {
  const year = game.release_date ? game.release_date.slice(0, 4) : null;

  // Genre/developer already live on `game` (resolved during the library
  // sync, same object the list row reads) — shown immediately, no need to
  // wait on the network round trip below just for these two rows.
  let infoHtml =
    `<div class="detail-info-row"><button class="cartridge-btn" id="gogCartridgeBtn">💾 Instalar en cartucho</button></div>` +
    tagsRow("Generos", game.genres);
  if (game.developers && game.developers.length) {
    infoHtml += infoRow("Developer", esc(game.developers.join(", ")));
  }
  infoHtml += infoRow("Plataforma", "GOG.com — DRM-free");
  if (year) infoHtml += infoRow("Lanzamiento", esc(year));
  infoHtml += infoRow("Peso de descarga", "Consultando...", "dpGogSize");
  infoHtml += infoRow("ID de producto", String(game.id));
  // Collapsed by default (no `open` attribute) — long GOG descriptions
  // otherwise push the screenshots gallery far below the fold before the
  // user gets to anything they actually clicked in for.
  infoHtml +=
    `<details class="detail-desc-toggle"><summary>Descripción</summary>` +
    `<div id="dpGogDesc">${loadingPlaceholder("Cargando descripción...")}</div></details>` +
    `<div id="dpGogShots"></div>`;

  document.getElementById("detailContent").innerHTML =
    detailHeaderShell(game.title, `<span>GOG${year ? " · " + esc(year) : ""}</span>`, headerImg(game.background_url)) +
    detailTabsShell([{ key: "info", label: "Info", initialHtml: infoHtml }]);
  installDetailTabSwitcher();
  document.getElementById("gogCartridgeBtn").onclick = () => openGogCartridgeModal(game.id);

  getCurrentWindow().setTitle(game.title + " — Tatu").catch(() => {});

  invoke("fetch_gog_extra_details", { appId: game.id })
    .then(details => {
      const descEl = document.getElementById("dpGogDesc");
      if (descEl) {
        descEl.innerHTML = details.description
          ? `<div class="detail-desc" style="white-space:pre-wrap;">${esc(details.description)}</div>`
          : "";
      }

      const shotsEl = document.getElementById("dpGogShots");
      if (shotsEl && details.screenshot_urls && details.screenshot_urls.length) {
        const items = details.screenshot_urls.map(url => cardGridItem(url, game.title));
        shotsEl.innerHTML = cardGrid("Capturas", items);
      }
    })
    .catch(e => {
      const descEl = document.getElementById("dpGogDesc");
      if (descEl) descEl.innerHTML = `<div class="detail-info-row" style="color:var(--danger)">No pude cargar la descripción: ${esc(String(e))}</div>`;
    });

  // Just `builds` + `repository` (gog_download's resolve_depot) — no
  // manifest, no chunks — so this is cheap enough to run every time the
  // detail window opens, letting the user compare weight across games
  // before committing to install a single one.
  invoke("gog_get_download_size", { productId: game.id, language: "en-US" })
    .then(info => {
      const el = document.getElementById("dpGogSize");
      if (el) el.querySelector(".detail-info-value").textContent = formatBytes(info.size);
    })
    .catch(e => {
      const el = document.getElementById("dpGogSize");
      if (el) {
        const value = el.querySelector(".detail-info-value");
        value.textContent = String(e);
        value.style.color = "#484f58";
      }
    });
}
