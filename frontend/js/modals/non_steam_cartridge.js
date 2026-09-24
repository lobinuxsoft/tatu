import { invoke, listen } from "../tauri.js";
import { state } from "../state.js";
import { esc, formatBytes } from "../utils.js";

// Same shared `#cartridgeOverlay`/`#cartridgeBody` the detail window's
// Steam and GOG cartridge modals already use (#328) — `detail.js` points
// `activeCartridgeClose` at `closeNonSteamCartridgeModal` while this
// source is showing, same switch it already does for GOG. The copy itself
// now reports real progress (`non_steam_copy_progress`, bytes) — live
// feedback: a static spinner made a fast small-game copy indistinguishable
// from a large one still running, same fix GOG's own download bar already
// needed for the same reason.
let copyActive = false;
let unlistenFns = [];

function stopListening() {
  for (const fn of unlistenFns) fn();
  unlistenFns = [];
}

export async function openNonSteamCartridgeModal(nonSteamId) {
  if (copyActive) return;
  const game = state.NS.find(g => g.id === nonSteamId);
  if (!game) return;
  document.getElementById("cartridgeOverlay").classList.remove("hidden");
  showDriveList(nonSteamId, game.name);
}

export function closeNonSteamCartridgeModal() {
  if (copyActive) return;
  document.getElementById("cartridgeOverlay").classList.add("hidden");
}

function body() {
  return document.getElementById("cartridgeBody");
}

async function showDriveList(nonSteamId, gameName) {
  const el = body();
  el.innerHTML = `<div class="loading"><div class="spinner"></div><br>Buscando discos...</div>`;
  try {
    const drives = await invoke("list_removable_drives");
    if (!drives.length) {
      el.innerHTML = `<div class="cartridge-warn">No se detectó ningún disco removible. Conectá uno y volvé a abrir esto.</div>`;
      return;
    }

    let html = "";
    for (const drive of drives) {
      const disabled = drive.read_only ? " disabled" : "";
      const tag = drive.read_only
        ? `<span class="drive-tag drive-tag-blank">🔒 Solo lectura</span>`
        : !drive.mount_point
          ? `<span class="drive-tag drive-tag-blank">Sin montar</span>`
          : `<span class="drive-tag drive-tag-ready">Listo</span>`;
      html +=
        `<div class="collection-row${disabled}" data-id="${esc(drive.id)}">` +
        `<span class="collection-name">${esc(drive.label || "Sin nombre")}</span>` +
        `<span class="collection-count">${formatBytes(drive.total_bytes)} ${tag}</span>` +
        `</div>`;
    }
    el.innerHTML = html;

    el.onclick = async e => {
      const row = e.target.closest(".collection-row");
      if (!row || row.classList.contains("disabled")) return;
      const drive = drives.find(d => d.id === row.dataset.id);
      if (!drive) return;

      let mountPoint = drive.mount_point;
      if (!mountPoint) {
        el.innerHTML = `<div class="loading"><div class="spinner"></div><br>Montando disco...</div>`;
        try {
          mountPoint = await invoke("mount_cartridge", { device: drive.id });
        } catch (err) {
          el.innerHTML =
            `<div class="cartridge-warn">No se pudo montar: ${esc(String(err))}</div>` +
            `<div class="cartridge-actions"><button class="cartridge-btn-secondary" id="nsCartBack">Volver</button></div>`;
          document.getElementById("nsCartBack").onclick = () => showDriveList(nonSteamId, gameName);
          return;
        }
      }
      startCopy(nonSteamId, gameName, mountPoint);
    };
  } catch (e) {
    el.innerHTML = `<div class="cartridge-warn">Error al leer discos: ${esc(String(e))}</div>`;
  }
}

// Listener is awaited into place before invoke() fires — the copy starts
// emitting on a background thread the instant the command returns, so
// registering it after the fact (like a fire-and-forget `.then`) can miss
// early progress events, same reasoning `gog_cartridge.js`'s own
// `startDownload` already documents for its download bar.
async function startCopy(nonSteamId, gameName, mountPoint) {
  const el = body();
  el.innerHTML =
    `<div class="ach-progress-wrap"><div class="ach-progress-bar" id="nsCopyBar" style="width:0%"></div>` +
    `<div class="ach-progress-text" id="nsCopyText">Preparando...</div></div>`;
  copyActive = true;
  stopListening();

  unlistenFns.push(
    await listen("non_steam_copy_progress", e => {
      const p = e.payload || {};
      const pct = p.total ? Math.min(100, Math.round((p.current / p.total) * 100)) : 0;
      const bar = document.getElementById("nsCopyBar");
      const text = document.getElementById("nsCopyText");
      if (bar) bar.style.width = pct + "%";
      if (text) text.textContent = `${formatBytes(p.current)} / ${formatBytes(p.total)}`;
    }),
  );

  try {
    const app = await invoke("install_non_steam_to_cartridge", { nonSteamId, mountPoint });
    copyActive = false;
    stopListening();
    el.innerHTML = `<div class="import-result import-result-ok">✓ "${esc(gameName)}" copiado a ${esc(mountPoint)}/${esc(app.exe_path.split("/")[0])}.</div>`;
  } catch (e) {
    copyActive = false;
    stopListening();
    el.innerHTML =
      `<div class="cartridge-warn">${esc(String(e))}</div>` +
      `<div class="cartridge-actions"><button class="cartridge-btn-secondary" id="nsCartRetryBack">Volver</button></div>`;
    document.getElementById("nsCartRetryBack").onclick = () => showDriveList(nonSteamId, gameName);
  }
}
