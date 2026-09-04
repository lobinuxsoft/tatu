import { invoke, listen } from "../tauri.js";
import { state } from "../state.js";
import { esc, formatBytes } from "../utils.js";

// GOG has no Steam-style "trigger and poll" install — Tatu itself does the
// download (content-system v2, #243), so this modal skips Steam's
// registration-check/version-choice/Goldberg steps entirely and goes
// straight from drive pick to a real progress bar fed by gog_cmd's events.
let unlistenFns = [];

// A real download keeps running on a background Rust thread regardless of
// what the UI does — live-reported (2026-08-30): clicking outside the
// modal closed it with no way to tell whether the download was still going.
// While this is true, the overlay refuses to close on its own; "Cancelar"
// (which actually stops the backend thread, not just hides the UI) is the
// only way out besides the download finishing on its own.
let downloadActive = false;

function stopListening() {
  for (const fn of unlistenFns) fn();
  unlistenFns = [];
}

export async function openGogCartridgeModal(gameId) {
  if (downloadActive) return;
  const game = state.GOG.find(g => g.id === gameId);
  if (!game) return;
  document.getElementById("cartridgeOverlay").classList.remove("hidden");
  showDriveList(gameId, game.title);
}

export function closeGogCartridgeModal() {
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
            `<div class="cartridge-actions"><button class="cartridge-btn-secondary" id="gogCartBack">Volver</button></div>`;
          document.getElementById("gogCartBack").onclick = () => showDriveList(gameId, gameName);
          return;
        }
      }
      showSizeConfirm(gameId, gameName, mountPoint);
    };
  } catch (e) {
    el.innerHTML = `<div class="cartridge-warn">Error al leer discos: ${esc(String(e))}</div>`;
  }
}

// Asks GOG for the depot's advertised size before touching the manifest or
// a single chunk — a couple of small JSON requests, not a real download —
// so the user can back out of something too big/too slow without having
// spent any real bandwidth on it.
async function showSizeConfirm(gameId, gameName, mountPoint) {
  const el = body();
  el.innerHTML = `<div class="loading"><div class="spinner"></div><br>Consultando tamaño...</div>`;
  try {
    const info = await invoke("gog_get_download_size", { productId: gameId, language: "en-US" });
    el.innerHTML =
      `<div class="cartridge-guide">"${esc(gameName)}"${info.version_name ? " " + esc(info.version_name) : ""} ` +
      `pesa <b>${formatBytes(info.size)}</b> (${formatBytes(info.compressed_size)} comprimido para descargar).</div>` +
      `<div class="cartridge-actions">` +
      `<button class="cartridge-btn-secondary" id="gogSizeBack">Volver</button>` +
      `<button class="cartridge-btn" id="gogSizeGo">Descargar</button>` +
      `</div>`;
    document.getElementById("gogSizeBack").onclick = () => showDriveList(gameId, gameName);
    document.getElementById("gogSizeGo").onclick = () => startDownload(gameId, gameName, mountPoint);
  } catch (e) {
    el.innerHTML =
      `<div class="cartridge-warn">No pude consultar el tamaño: ${esc(String(e))}</div>` +
      `<div class="cartridge-actions"><button class="cartridge-btn-secondary" id="gogSizeBack2">Volver</button></div>`;
    document.getElementById("gogSizeBack2").onclick = () => showDriveList(gameId, gameName);
  }
}

// Listeners are awaited into place before invoke() fires — the download
// starts emitting on a background thread the instant the command returns,
// registering them after the fact (like a fire-and-forget `.then`) can miss
// the very first progress events.
async function startDownload(gameId, gameName, mountPoint) {
  const el = body();
  el.innerHTML =
    `<div class="ach-progress-wrap"><div class="ach-progress-bar" id="gogDlBar" style="width:0%"></div>` +
    `<div class="ach-progress-text" id="gogDlText">Preparando...</div></div>` +
    `<div class="gog-dl-path" id="gogDlPath"></div>` +
    `<div class="cartridge-actions"><button class="cartridge-btn-secondary" id="gogDlCancel">Cancelar</button></div>`;
  document.getElementById("gogDlCancel").onclick = () => invoke("gog_cancel_download");

  stopListening();
  downloadActive = true;

  unlistenFns.push(
    await listen("gog_download_started", e => {
      const p = e.payload || {};
      const text = document.getElementById("gogDlText");
      if (text) text.textContent = `${gameName} ${p.version_name || ""}`.trim();
    }),
  );
  unlistenFns.push(
    await listen("gog_download_progress", e => {
      const p = e.payload || {};
      const pct = p.total ? Math.round((p.current / p.total) * 100) : 0;
      const bar = document.getElementById("gogDlBar");
      const text = document.getElementById("gogDlText");
      const path = document.getElementById("gogDlPath");
      if (bar) bar.style.width = pct + "%";
      if (text) text.textContent = `${p.current}/${p.total}`;
      if (path) path.textContent = p.path || "";
    }),
  );
  unlistenFns.push(
    await listen("gog_download_done", () => {
      downloadActive = false;
      stopListening();
      el.innerHTML = `<div class="import-result import-result-ok">✓ "${esc(gameName)}" instalado en ${esc(mountPoint)}/GOG.</div>`;
    }),
  );
  unlistenFns.push(
    await listen("gog_download_cancelled", () => {
      downloadActive = false;
      stopListening();
      el.innerHTML =
        `<div class="cartridge-guide">Descarga de "${esc(gameName)}" cancelada.</div>` +
        `<div class="cartridge-actions"><button class="cartridge-btn-secondary" id="gogDlCancelBack">Volver</button></div>`;
      document.getElementById("gogDlCancelBack").onclick = () => showDriveList(gameId, gameName);
    }),
  );
  unlistenFns.push(
    await listen("gog_download_error", e => {
      downloadActive = false;
      stopListening();
      el.innerHTML =
        `<div class="cartridge-warn">${esc(String(e.payload))}</div>` +
        `<div class="cartridge-actions"><button class="cartridge-btn-secondary" id="gogDlBack">Volver</button></div>`;
      document.getElementById("gogDlBack").onclick = () => showDriveList(gameId, gameName);
    }),
  );

  try {
    await invoke("gog_download_game", {
      productId: gameId,
      gameName,
      mountPoint,
      language: "en-US",
    });
  } catch (e) {
    downloadActive = false;
    stopListening();
    el.innerHTML = `<div class="cartridge-warn">${esc(String(e))}</div>`;
  }
}
