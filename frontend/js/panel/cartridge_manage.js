import { invoke } from "../tauri.js";
import { esc, formatBytes } from "../utils.js";

// Short labels for the same Preservability kinds the Steam tab's filter
// buttons already use (see index.html's #presRow) — no need for a second
// vocabulary here.
const PRES_LABEL = {
  trivial: "💾 Trivial",
  easy: "🔧 Easy (Goldberg)",
  alternative: "🍬 GOG",
  removed: "💧 Removido",
  hard: "🧱 Difícil",
  unknown: "? Desconocido",
};

function body() {
  return document.getElementById("cartridgeManageBody");
}

// Entry point, called every time the "Cartucho" tab is opened — a fresh
// scan each time rather than a cached one, since what's physically plugged
// in can change between visits.
export async function openCartridgeManagePanel() {
  await showDriveList();
}

async function showDriveList() {
  const el = body();
  el.innerHTML = `<div class="loading"><div class="spinner"></div><br>Buscando discos...</div>`;
  try {
    const drives = await invoke("list_removable_drives");
    const ready = [];
    for (const drive of drives) {
      if (drive.mount_point && (await invoke("has_cartridge_structure", { mountPoint: drive.mount_point }))) {
        ready.push(drive);
      }
    }

    if (!ready.length) {
      el.innerHTML =
        `<div class="cartridge-warn">No hay ningún cartucho ya armado conectado. ` +
        `Instalá al menos un juego en un cartucho desde su ficha (botón "Instalar en cartucho") primero.</div>`;
      return;
    }

    if (ready.length === 1) {
      await showCartridge(ready[0]);
      return;
    }

    let html = `<div class="cartridge-guide">Elegí el cartucho a preparar:</div>`;
    for (const drive of ready) {
      html +=
        `<div class="collection-row" data-id="${esc(drive.id)}">` +
        `<span class="collection-name">${esc(drive.label || "Sin nombre")}</span>` +
        `<span class="collection-count">${formatBytes(drive.total_bytes)}</span></div>`;
    }
    el.innerHTML = html;
    el.onclick = e => {
      const row = e.target.closest(".collection-row");
      if (!row) return;
      const picked = ready.find(d => d.id === row.dataset.id);
      if (picked) showCartridge(picked);
    };
  } catch (e) {
    el.innerHTML = `<div class="cartridge-warn">Error al leer discos: ${esc(String(e))}</div>`;
  }
}

async function showCartridge(drive) {
  const el = body();
  el.innerHTML = `<div class="loading"><div class="spinner"></div><br>Leyendo cartucho...</div>`;
  try {
    const apps = await invoke("list_cartridge_apps", { mountPoint: drive.mount_point });
    renderCartridge(drive, apps);
  } catch (e) {
    el.innerHTML = `<div class="cartridge-warn">Error al leer el cartucho: ${esc(String(e))}</div>`;
  }
}

function renderCartridge(drive, apps) {
  const el = body();
  const rows = apps.length
    ? apps
        .map(app => {
          const pres = PRES_LABEL[app.preservability && app.preservability.kind] || PRES_LABEL.unknown;
          const standalone = app.standalone ? " · standalone" : "";
          return (
            `<div class="collection-row"><span class="collection-name">${esc(app.name)}</span>` +
            `<span class="collection-count">${pres}${standalone}</span></div>`
          );
        })
        .join("")
    : `<div class="cartridge-warn">Este cartucho todavía no tiene juegos instalados.</div>`;

  el.innerHTML =
    `<div class="cartridge-guide">Cartucho: <b>${esc(drive.label || drive.id)}</b> ` +
    `(${formatBytes(drive.total_bytes)}) — ${apps.length} juego(s)</div>` +
    rows +
    `<div class="cartridge-actions">` +
    `<button class="cartridge-btn-secondary" id="cartManageBack">Volver a discos</button>` +
    `<button class="cartridge-btn" id="cartPrepareBtn">Preparar launcher</button>` +
    `</div>` +
    `<div id="cartPrepareResult"></div>`;

  document.getElementById("cartManageBack").onclick = () => showDriveList();
  document.getElementById("cartPrepareBtn").onclick = () => prepareLauncher(drive, apps);
}

// Everything that's shared by the WHOLE cartridge rather than tied to one
// game's install — the launcher binary, the Linux runtime, and every app's
// art/description — batched here instead of firing per-game at install
// time (finishInstall in cartridge.js only does Goldberg injection now).
async function prepareLauncher(drive, apps) {
  const result = document.getElementById("cartPrepareResult");
  const mountPoint = drive.mount_point;
  const setStatus = msg => {
    result.innerHTML = `<div class="loading"><div class="spinner"></div><br>${esc(msg)}</div>`;
  };

  try {
    setStatus("Copiando el launcher (Linux + Windows)...");
    await invoke("install_launcher_binaries", { mountPoint });

    if (apps.some(a => a.preservability && a.preservability.kind === "easy")) {
      setStatus("Preparando runtime de Linux (Proton)...");
      await invoke("bundle_linux_runtime", { mountPoint });
    }

    for (let i = 0; i < apps.length; i++) {
      const app = apps[i];
      setStatus(`Bajando arte y descripción (${i + 1}/${apps.length}): ${app.name}`);
      await Promise.all([
        invoke("fetch_cartridge_art", { appId: app.app_id, mountPoint }).catch(() => {}),
        invoke("fetch_cartridge_description", { appId: app.app_id, mountPoint }).catch(() => {}),
      ]);
    }

    result.innerHTML =
      `<div class="import-result import-result-ok">✓ Cartucho listo — launcher instalado ` +
      `para Linux y Windows, ${apps.length} juego(s) preparados.</div>`;
  } catch (e) {
    result.innerHTML = `<div class="cartridge-warn">No se pudo terminar de preparar el cartucho: ${esc(String(e))}</div>`;
  }
}
