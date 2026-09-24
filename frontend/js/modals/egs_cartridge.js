import { invoke, listen } from "../tauri.js";
import { state } from "../state.js";
import { esc, formatBytes } from "../utils.js";

// Same shape as gog_cartridge.js (#243): Tatu itself does the download
// (manifest+chunk protocol, #344), so this modal skips Steam's own
// trigger-and-poll install and goes straight from drive pick to a real
// progress bar fed by egs_cmd's events.
let unlistenFns = [];
let downloadActive = false;

function stopListening() {
  for (const fn of unlistenFns) fn();
  unlistenFns = [];
}

export async function openEgsCartridgeModal(gameId) {
  if (downloadActive) return;
  const game = state.EGS.find(g => g.id === gameId);
  if (!game) return;
  document.getElementById("cartridgeOverlay").classList.remove("hidden");
  showDriveList(gameId, game.title);
}

export function closeEgsCartridgeModal() {
  if (downloadActive) return;
  stopListening();
  document.getElementById("cartridgeOverlay").classList.add("hidden");
}

function body() {
  return document.getElementById("cartridgeBody");
}

async function showDriveList(gameId, gameName) {
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
            `<div class="cartridge-actions"><button class="cartridge-btn-secondary" id="egsCartBack">Volver</button></div>`;
          document.getElementById("egsCartBack").onclick = () => showDriveList(gameId, gameName);
          return;
        }
      }
      showSizeConfirm(gameId, gameName, mountPoint);
    };
  } catch (e) {
    el.innerHTML = `<div class="cartridge-warn">Error al leer discos: ${esc(String(e))}</div>`;
  }
}

// Fetches and parses the real manifest (no chunk bytes yet) so the user can
// see the actual size before committing to a download — same reasoning
// gog_cartridge.js's own size-confirm step uses.
async function showSizeConfirm(gameId, gameName, mountPoint) {
  const el = body();
  el.innerHTML = `<div class="loading"><div class="spinner"></div><br>Consultando tamaño...</div>`;
  try {
    const info = await invoke("egs_get_download_size", { appId: gameId });
    el.innerHTML =
      `<div class="cartridge-guide">"${esc(gameName)}"${info.build_version ? " " + esc(info.build_version) : ""} ` +
      `pesa <b>${formatBytes(info.total_size)}</b>.</div>` +
      `<div class="cartridge-actions">` +
      `<button class="cartridge-btn-secondary" id="egsSizeBack">Volver</button>` +
      `<button class="cartridge-btn" id="egsSizeGo">Descargar</button>` +
      `</div>`;
    document.getElementById("egsSizeBack").onclick = () => showDriveList(gameId, gameName);
    document.getElementById("egsSizeGo").onclick = () => startDownload(gameId, gameName, mountPoint);
  } catch (e) {
    el.innerHTML =
      `<div class="cartridge-warn">No pude consultar el tamaño: ${esc(String(e))}</div>` +
      `<div class="cartridge-actions"><button class="cartridge-btn-secondary" id="egsSizeBack2">Volver</button></div>`;
    document.getElementById("egsSizeBack2").onclick = () => showDriveList(gameId, gameName);
  }
}

// Listeners are awaited into place before invoke() fires — same ordering
// gog_cartridge.js's startDownload needs, for the same reason: the download
// starts emitting on a background thread the instant the command returns.
async function startDownload(gameId, gameName, mountPoint) {
  const el = body();
  el.innerHTML =
    `<div class="ach-progress-wrap"><div class="ach-progress-bar" id="egsDlBar" style="width:0%"></div>` +
    `<div class="ach-progress-text" id="egsDlText">Preparando...</div></div>` +
    `<div class="gog-dl-path" id="egsDlPath"></div>` +
    `<div class="cartridge-actions"><button class="cartridge-btn-secondary" id="egsDlCancel">Cancelar</button></div>`;
  document.getElementById("egsDlCancel").onclick = () => invoke("egs_cancel_download");

  stopListening();
  downloadActive = true;

  unlistenFns.push(
    await listen("egs_download_started", e => {
      const p = e.payload || {};
      const text = document.getElementById("egsDlText");
      if (text) text.textContent = `${gameName} ${p.build_version || ""}`.trim();
    }),
  );
  unlistenFns.push(
    await listen("egs_download_progress", e => {
      const p = e.payload || {};
      const pct = p.total ? Math.round((p.current / p.total) * 100) : 0;
      const bar = document.getElementById("egsDlBar");
      const text = document.getElementById("egsDlText");
      const path = document.getElementById("egsDlPath");
      if (bar) bar.style.width = pct + "%";
      if (text) text.textContent = `${p.current}/${p.total}`;
      if (path) path.textContent = p.path || "";
    }),
  );
  unlistenFns.push(
    await listen("egs_download_done", () => {
      downloadActive = false;
      stopListening();
      el.innerHTML = `<div class="import-result import-result-ok">✓ "${esc(gameName)}" instalado en ${esc(mountPoint)}/EGS.</div>`;
    }),
  );
  unlistenFns.push(
    await listen("egs_download_cancelled", () => {
      downloadActive = false;
      stopListening();
      el.innerHTML =
        `<div class="cartridge-guide">Descarga de "${esc(gameName)}" cancelada.</div>` +
        `<div class="cartridge-actions"><button class="cartridge-btn-secondary" id="egsDlCancelBack">Volver</button></div>`;
      document.getElementById("egsDlCancelBack").onclick = () => showDriveList(gameId, gameName);
    }),
  );
  unlistenFns.push(
    await listen("egs_download_error", e => {
      downloadActive = false;
      stopListening();
      el.innerHTML =
        `<div class="cartridge-warn">${esc(String(e.payload))}</div>` +
        `<div class="cartridge-actions"><button class="cartridge-btn-secondary" id="egsDlBack">Volver</button></div>`;
      document.getElementById("egsDlBack").onclick = () => showDriveList(gameId, gameName);
    }),
  );

  try {
    await invoke("egs_download_game", { appId: gameId, mountPoint });
  } catch (e) {
    downloadActive = false;
    stopListening();
    el.innerHTML = `<div class="cartridge-warn">${esc(String(e))}</div>`;
  }
}
