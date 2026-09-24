import { invoke, getCurrentWindow } from "../tauri.js";
import { esc } from "../utils.js";
import { openNonSteamCartridgeModal } from "../modals/non_steam_cartridge.js";
import { artworkTabHtml, installArtworkTabHandlers } from "./artwork_tab.js";
import {
  detailHeaderShell,
  headerImg,
  infoRow,
  cardGridItem,
  cardGrid,
  detailTabsShell,
  installDetailTabSwitcher,
  loadingPlaceholder,
} from "./detail_template.js";

// Same shared window template as Steam/GOG (#328, live feedback: a
// Non-Steam entry looked nothing like the other two) — one "Info" tab,
// same reasoning as GOG's renderer: no achievements/cards/cheats, those
// are real Steam APIs a shortcut has no equivalent of. The one thing this
// source needs that neither other one does is the SteamAppID field itself
// — Steam/GOG both already know their own id, a shortcut has none until
// the user assigns one.
export function renderNonSteamDetail(game) {
  let infoHtml =
    `<div class="detail-info-row"><button class="cartridge-btn" id="nsCartridgeBtn">💾 Instalar en cartucho</button></div>` +
    appIdRow(game) +
    infoRow("Ejecutable", esc(game.exe)) +
    installRootRow(game) +
    `<div id="dpNsPreview"></div>`;

  document.getElementById("detailContent").innerHTML =
    detailHeaderShell(game.name, `<span>Non-Steam</span>`, headerImg("")) +
    detailTabsShell([
      { key: "info", label: "Info", initialHtml: infoHtml },
      { key: "arte", label: "Arte", initialHtml: artworkTabHtml() },
    ]);
  installDetailTabSwitcher();
  document.getElementById("nsCartridgeBtn").onclick = () => openNonSteamCartridgeModal(game.id);
  installArtworkTabHandlers(game.id, game.name);
  installAppIdHandlers(game);
  installInstallRootHandlers(game);

  getCurrentWindow().setTitle(game.name + " — Tatu").catch(() => {});

  if (game.steam_app_id) loadPreview(game.steam_app_id);
}

function appIdRow(game) {
  return (
    `<div class="detail-info-row">` +
    `<span class="detail-info-label">SteamAppID</span>` +
    `<span class="detail-info-value">` +
    `<input type="number" id="nsAppIdInput" min="1" placeholder="ej. 379720" value="${game.steam_app_id || ""}">` +
    ` <button class="cartridge-btn-secondary" id="nsAppIdSave">Guardar</button>` +
    `</span>` +
    `</div>` +
    `<div id="dpNsAppIdMsg"></div>`
  );
}

function installRootOf(game) {
  return game.install_root_override || game.start_dir || "";
}

// Steam's own "Start In" for a shortcut is frequently just the exe's own
// folder (an Unreal Engine game's `<Root>/<Game>/Binaries/Win64/`, not
// `<Root>/<Game>/`) — live feedback: copying only that left every sibling
// folder the game actually needs behind. This lets the user point at the
// real one; the native folder picker beats typing a path by hand.
function installRootRow(game) {
  return (
    `<div class="detail-info-row">` +
    `<span class="detail-info-label">Carpeta del juego</span>` +
    `<span class="detail-info-value">` +
    `<input type="text" id="nsRootInput" value="${esc(installRootOf(game))}" style="width:22rem">` +
    ` <button class="cartridge-btn-secondary" id="nsRootBrowse">📁</button>` +
    ` <button class="cartridge-btn-secondary" id="nsRootSave">Guardar</button>` +
    `</span>` +
    `</div>` +
    `<div id="dpNsRootMsg"></div>`
  );
}

function installInstallRootHandlers(game) {
  document.getElementById("nsRootBrowse").onclick = async () => {
    const input = document.getElementById("nsRootInput");
    try {
      const picked = await invoke("pick_non_steam_folder", { startIn: input.value || null });
      if (picked) input.value = picked;
    } catch (_) {
      // User cancelled the dialog — nothing to report.
    }
  };
  document.getElementById("nsRootSave").onclick = async () => {
    const input = document.getElementById("nsRootInput");
    const msgEl = document.getElementById("dpNsRootMsg");
    try {
      await invoke("set_non_steam_install_root", { nonSteamId: game.id, path: input.value.trim() || null });
      game.install_root_override = input.value.trim() || null;
      msgEl.innerHTML = `<div class="detail-info-row" style="color:var(--success)">Guardado.</div>`;
    } catch (e) {
      msgEl.innerHTML = `<div class="detail-info-row" style="color:var(--danger)">No pude guardar: ${esc(String(e))}</div>`;
    }
  };
}

// Confirming the id actually matches this game is the whole point (#328,
// live feedback: setear un AppID a ciegas es peor que no tener nada) — a
// wrong id shows the WRONG game's splash art/description right here,
// before it's ever baked into a cartridge.
function installAppIdHandlers(game) {
  document.getElementById("nsAppIdSave").onclick = async () => {
    const input = document.getElementById("nsAppIdInput");
    const raw = input.value.trim();
    const steamAppId = raw ? parseInt(raw, 10) : null;
    const msgEl = document.getElementById("dpNsAppIdMsg");
    try {
      await invoke("set_non_steam_appid", { nonSteamId: game.id, steamAppId });
      game.steam_app_id = steamAppId;
      msgEl.innerHTML = "";
      const previewEl = document.getElementById("dpNsPreview");
      if (previewEl) previewEl.innerHTML = "";
      if (steamAppId) loadPreview(steamAppId);
    } catch (e) {
      msgEl.innerHTML = `<div class="detail-info-row" style="color:var(--danger)">No pude guardar: ${esc(String(e))}</div>`;
    }
  };
}

function loadPreview(steamAppId) {
  const el = document.getElementById("dpNsPreview");
  if (!el) return;
  el.innerHTML = loadingPlaceholder("Buscando en Steam / SteamGridDB...");

  invoke("get_non_steam_preview", { steamAppId })
    .then(preview => {
      const current = document.getElementById("dpNsPreview");
      if (!current) return;

      if (preview.grid_url) {
        const img = document.getElementById("dpHeaderImg");
        if (img) img.innerHTML = headerImg(preview.grid_url);
      }

      let html = "";
      if (preview.name) {
        html += `<div class="detail-info-row"><span class="detail-info-label">Steam dice</span><span class="detail-info-value">${esc(preview.name)}</span></div>`;
      }
      if (preview.description) {
        html += `<div class="detail-desc" style="white-space:pre-wrap;">${esc(preview.description)}</div>`;
      }
      if (preview.screenshot_urls && preview.screenshot_urls.length) {
        const items = preview.screenshot_urls.map(url => cardGridItem(url, preview.name || ""));
        html += cardGrid("Capturas", items);
      }
      if (!html) {
        html = `<div class="detail-info-row" style="color:var(--fg-dim)">Ese AppID no devolvió datos — ¿seguro que es el correcto?</div>`;
      }
      current.innerHTML = html;
    })
    .catch(e => {
      const current = document.getElementById("dpNsPreview");
      if (current) current.innerHTML = `<div class="detail-info-row" style="color:var(--danger)">No pude consultar Steam: ${esc(String(e))}</div>`;
    });
}
