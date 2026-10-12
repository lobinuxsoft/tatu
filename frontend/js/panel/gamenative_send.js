import { invoke, listen } from "../tauri.js";
import { esc, formatBytes } from "../utils.js";
import { loadingPlaceholder } from "./detail_template.js";
import { showDevicePicker } from "./android_devices.js";

// "Enviar a Android" (GameNative tab), in its own popup: device →
// destination folder → copy + verify (the backend only renames the copy into
// place once every file's SHA-256 matches) → the one step left on the phone.
// Nothing touches adb until the button is clicked.

// One send at a time for the whole window: the backend has a single cancel
// slot. The popup can't be closed mid-send (Cancelar is right there).
let sendActive = false;
let unlisten = null;

const PHASE_LABEL = { pushing: "Copiando", verifying: "Verificando" };


export function sendSectionHtml() {
  return (
    `<div class="detail-info-row"><span class="detail-info-label">Enviar al dispositivo</span></div>` +
    `<div class="detail-info-row">` +
    `<button class="cartridge-btn" id="gnSendBtn">📱 Enviar a Android</button>` +
    `<span class="detail-info-value">Copia el juego por USB (adb) al dispositivo, verificado archivo por archivo.</span>` +
    `</div>`
  );
}

export function installSendSection(source, id, gameName) {
  const btn = document.getElementById("gnSendBtn");
  if (btn) btn.onclick = () => openSendPopup({ source, id, gameName });
}

// Every step renders into `ctx.el` (the popup's body) and looks its own
// controls up inside it.
function $(ctx, selector) {
  return ctx.el.querySelector(selector);
}

async function openSendPopup(ctx) {
  if (sendActive) return;
  const overlay = document.getElementById("gnSendOverlay");
  overlay.classList.remove("hidden");
  document.getElementById("gnSendClose").onclick = closeSendPopup;
  overlay.onclick = e => {
    if (e.target === overlay) closeSendPopup();
  };

  ctx.el = document.getElementById("gnSendBody");
  ctx.el.onclick = null;
  ctx.el.innerHTML = loadingPlaceholder("Buscando la carpeta del juego...");
  try {
    ctx.folder = await invoke("android_game_folder", { source: ctx.source, id: ctx.id });
    pickDevice(ctx);
  } catch (e) {
    ctx.el.innerHTML = `<div class="cartridge-warn">${esc(String(e))}</div>`;
  }
}

function closeSendPopup() {
  if (sendActive) return;
  document.getElementById("gnSendOverlay").classList.add("hidden");
}

function pickDevice(ctx) {
  showDevicePicker(ctx.el, device => showDestination(ctx, device));
}

function showDestination(ctx, device) {
  ctx.el.onclick = null;
  ctx.el.innerHTML =
    `<div class="cartridge-guide">Origen: ${esc(ctx.folder.path)}<br>Dispositivo: ${esc(device.model || device.serial)}</div>` +
    `<div class="art-search-row"><input type="text" data-gn="root" value="${esc(ctx.folder.remote_root)}"></div>` +
    `<div class="modal-hint" data-gn="dest"></div>` +
    `<div class="cartridge-actions">` +
    `<button class="cartridge-btn" data-gn="send">📱 Enviar</button>` +
    `<button class="cartridge-btn-secondary" data-gn="back">Volver</button>` +
    `</div>`;

  const input = $(ctx, '[data-gn="root"]');
  const updateDest = () => {
    $(ctx, '[data-gn="dest"]').textContent =
      `Se va a copiar a ${input.value.trim().replace(/\/+$/, "")}/${ctx.folder.name}`;
  };
  input.oninput = updateDest;
  updateDest();
  $(ctx, '[data-gn="back"]').onclick = () => pickDevice(ctx);
  $(ctx, '[data-gn="send"]').onclick = () => startSend(ctx, device, input.value);
}

// Listener is awaited into place before invoke() fires — the send starts
// emitting on a background thread right away, same reasoning
// `non_steam_cartridge.js`'s `startCopy` documents for its progress bar.
async function startSend(ctx, device, remoteRoot) {
  ctx.el.innerHTML =
    `<div class="ach-progress-wrap"><div class="ach-progress-bar" data-gn="bar" style="width:0%"></div>` +
    `<div class="ach-progress-text" data-gn="text">Preparando...</div></div>` +
    `<div class="cartridge-actions"><button class="cartridge-btn-secondary" data-gn="cancel">Cancelar</button></div>`;
  $(ctx, '[data-gn="cancel"]').onclick = e => {
    e.target.disabled = true;
    e.target.textContent = "Cancelando...";
    invoke("android_cancel_send").catch(() => {});
  };

  sendActive = true;
  if (unlisten) unlisten();
  unlisten = await listen("android_send_progress", e => {
    const p = e.payload || {};
    const pct = p.total ? Math.min(100, Math.round((p.current / p.total) * 100)) : 100;
    const bar = $(ctx, '[data-gn="bar"]');
    const text = $(ctx, '[data-gn="text"]');
    if (bar) bar.style.width = pct + "%";
    if (!text) return;
    // This machine usually finishes hashing first; the rest is the phone's side.
    text.textContent =
      p.phase === "verifying" && p.current >= p.total
        ? "Verificando la copia en el teléfono..."
        : `${PHASE_LABEL[p.phase] || ""}: ${formatBytes(p.current)} / ${formatBytes(p.total)}`;
  });

  try {
    const finalPath = await invoke("android_send_game", {
      source: ctx.source,
      id: ctx.id,
      serial: device.serial,
      remoteRoot,
    });
    showManualStep(ctx, finalPath);
  } catch (e) {
    ctx.el.innerHTML =
      `<div class="cartridge-warn">${esc(String(e))}</div>` +
      `<div class="cartridge-actions"><button class="cartridge-btn-secondary" data-gn="retry">Volver</button></div>`;
    $(ctx, '[data-gn="retry"]').onclick = () => {
      ctx.folder.remote_root = remoteRoot;
      showDestination(ctx, device);
    };
  } finally {
    sendActive = false;
    if (unlisten) unlisten();
    unlisten = null;
  }
}

// GameNative has no way for a PC to register a folder in its library (no
// exported API, and its database is app-private), so the last step is
// always the user's: add the folder from the phone. Once added, a Steam
// game the user owns shows up as a Steam game — the folder is named after
// its `installdir` on purpose.
function showManualStep(ctx, finalPath) {
  ctx.el.innerHTML =
    `<div class="import-result import-result-ok">✓ "${esc(ctx.gameName)}" se copió y verificó en el teléfono.</div>` +
    `<div class="cartridge-guide"><b>Falta un paso en el teléfono</b> para que aparezca en GameNative:` +
    `<ol>` +
    `<li>Abrí GameNative y andá a la biblioteca.</li>` +
    `<li>Tocá <b>Añadir juego personalizado</b>.</li>` +
    `<li>Elegí la carpeta <b>${esc(friendlyPath(finalPath))}</b> y, si te lo pide, dale permiso de acceso a los archivos.</li>` +
    `</ol>` +
    (ctx.source === "steam" ? `Como es un juego de tu cuenta de Steam, GameNative lo va a mostrar como juego de Steam.<br>` : "") +
    `<span style="color:var(--fg-dim)">${esc(finalPath)}</span></div>` +
    `<div class="cartridge-actions"><button class="cartridge-btn" data-gn="done">Entendido</button></div>`;
  $(ctx, '[data-gn="done"]').onclick = closeSendPopup;
}

// How Android's own folder picker names these places, instead of raw paths.
function friendlyPath(path) {
  const internal = path.match(/^\/(?:sdcard|storage\/emulated\/0)(\/.*)?$/);
  if (internal) return ["Almacenamiento interno", ...(internal[1] || "").split("/").filter(Boolean)].join(" › ");
  const card = path.match(/^\/storage\/([^/]+)(\/.*)?$/);
  if (card) return [`Tarjeta SD (${card[1]})`, ...(card[2] || "").split("/").filter(Boolean)].join(" › ");
  return path;
}
