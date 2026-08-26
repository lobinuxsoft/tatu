import { invoke } from "../tauri.js";
import { state } from "../state.js";
import { esc, formatBytes } from "../utils.js";

// Guards against a leaked timer if the modal is closed mid-poll or a second
// install is started on top of one already running.
let pollTimer = null;

export function openCartridgeModal(gameId) {
  const game = state.G.find(g => g.id === gameId);
  if (!game) return;
  document.getElementById("cartridgeOverlay").classList.remove("hidden");
  showDriveList(gameId, game.name);
}

export function closeCartridgeModal() {
  stopPolling();
  document.getElementById("cartridgeOverlay").classList.add("hidden");
}

function stopPolling() {
  if (pollTimer) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
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

    // has_cartridge_structure needs a mount point, so an unmounted drive is
    // shown (the user should still see it's there) but reported as such
    // rather than silently treated as blank.
    const rows = await Promise.all(
      drives.map(async drive => ({
        drive,
        ready: drive.mount_point
          ? await invoke("has_cartridge_structure", { mountPoint: drive.mount_point })
          : false,
      })),
    );

    let html = "";
    for (const { drive, ready } of rows) {
      // Read-only blocks both formatting (blank drive) and installing
      // (steamapps/ needs write access), so it's a dead end either way.
      const disabled = !drive.mount_point || drive.read_only ? " disabled" : "";
      const tag = drive.read_only
        ? `<span class="drive-tag drive-tag-blank">🔒 Solo lectura</span>`
        : !drive.mount_point
          ? `<span class="drive-tag drive-tag-blank">Sin montar</span>`
          : ready
            ? `<span class="drive-tag drive-tag-ready">Cartucho existente</span>`
            : `<span class="drive-tag drive-tag-blank">Vacío</span>`;
      html +=
        `<div class="collection-row${disabled}" data-id="${esc(drive.id)}">` +
        `<span class="collection-name">${esc(drive.label || "Sin nombre")}</span>` +
        `<span class="collection-count">${formatBytes(drive.total_bytes)} ${tag}</span>` +
        `</div>`;
    }
    el.innerHTML = html;

    el.onclick = e => {
      const row = e.target.closest(".collection-row");
      if (!row || row.classList.contains("disabled")) return;
      const picked = rows.find(r => r.drive.id === row.dataset.id);
      if (!picked) return;
      if (picked.ready) {
        showRegistrationCheck(gameId, gameName, picked.drive.mount_point);
      } else {
        showFormatConfirm(gameId, gameName, picked.drive);
      }
    };
  } catch (e) {
    el.innerHTML = `<div class="cartridge-warn">Error al leer discos: ${esc(String(e))}</div>`;
  }
}

function showFormatConfirm(gameId, gameName, drive) {
  const el = body();
  el.innerHTML =
    `<div class="cartridge-warn">Vas a formatear "<b>${esc(drive.label || drive.id)}</b>" ` +
    `(${formatBytes(drive.total_bytes)}) como cartucho. Esto BORRA todo su contenido actual, sin vuelta atrás.</div>` +
    `<div class="cartridge-actions">` +
    `<button class="cartridge-btn-secondary" id="cartFormatCancel">Volver</button>` +
    `<button class="cartridge-btn" id="cartFormatGo">Formatear</button>` +
    `</div>`;

  document.getElementById("cartFormatCancel").onclick = () => showDriveList(gameId, gameName);
  document.getElementById("cartFormatGo").onclick = async () => {
    el.innerHTML = `<div class="loading"><div class="spinner"></div><br>Formateando...</div>`;
    try {
      await invoke("format_as_cartridge", {
        device: drive.id,
        expectedLabel: drive.label,
        expectedBytes: drive.total_bytes,
      });
      const fresh = (await invoke("list_removable_drives")).find(d => d.id === drive.id);
      if (!fresh || !fresh.mount_point) {
        el.innerHTML = `<div class="cartridge-warn">Formateado, pero no encontré el punto de montaje. Reconectá el disco e intentá de nuevo.</div>`;
        return;
      }
      showRegistrationCheck(gameId, gameName, fresh.mount_point);
    } catch (e) {
      // format_as_cartridge isn't registered at all on Windows yet (#194) —
      // Tauri's own "command not found" error is how that surfaces here.
      const raw = String(e);
      const msg = /not found|unknown command/i.test(raw)
        ? "Formatear todavía no está soportado en Windows (ver #194)."
        : raw;
      el.innerHTML =
        `<div class="cartridge-warn">${esc(msg)}</div>` +
        `<div class="cartridge-actions"><button class="cartridge-btn-secondary" id="cartFormatBack">Volver</button></div>`;
      document.getElementById("cartFormatBack").onclick = () => showDriveList(gameId, gameName);
    }
  };
}

async function showRegistrationCheck(gameId, gameName, mountPoint) {
  const el = body();
  el.innerHTML = `<div class="loading"><div class="spinner"></div><br>Verificando registro en Steam...</div>`;
  try {
    const registered = await invoke("is_registered_library", { mountPoint });
    if (registered) {
      showInstall(gameId, gameName, mountPoint);
    } else {
      showRegistrationGuide(gameId, gameName, mountPoint);
    }
  } catch (e) {
    el.innerHTML = `<div class="cartridge-warn">Error: ${esc(String(e))}</div>`;
  }
}

function showRegistrationGuide(gameId, gameName, mountPoint) {
  const el = body();
  el.innerHTML =
    `<div class="cartridge-guide">Este disco todavía no está registrado como librería de Steam. ` +
    `En Steam: <b>Configuración → Almacenamiento → Agregar unidad</b> → elegí este disco. ` +
    `Volvé acá cuando termines.</div>` +
    `<div class="cartridge-actions">` +
    `<button class="cartridge-btn-secondary" id="cartRegBack">Volver</button>` +
    `<button class="cartridge-btn" id="cartRegDone">Ya lo hice</button>` +
    `</div>`;
  document.getElementById("cartRegBack").onclick = () => showDriveList(gameId, gameName);
  document.getElementById("cartRegDone").onclick = () =>
    showRegistrationCheck(gameId, gameName, mountPoint);
}

function showInstall(gameId, gameName, mountPoint) {
  const el = body();
  el.innerHTML = `<div class="loading"><div class="spinner"></div><br>Iniciando instalación en Steam...</div>`;

  invoke("trigger_install", { appId: gameId })
    .then(() => {
      el.innerHTML =
        `<div class="loading"><div class="spinner"></div><br>Instalando "${esc(gameName)}"... ` +
        `seguí el progreso en la ventana de Steam.</div>`;
      stopPolling();
      pollTimer = setInterval(async () => {
        try {
          const done = await invoke("poll_install_status", { appId: gameId, mountPoint });
          if (done) {
            stopPolling();
            await finishInstall(gameId, gameName, mountPoint);
          }
        } catch (e) {
          stopPolling();
          el.innerHTML = `<div class="cartridge-warn">Error verificando la instalación: ${esc(String(e))}</div>`;
        }
      }, 2500);
    })
    .catch(e => {
      el.innerHTML = `<div class="cartridge-warn">${esc(String(e))}</div>`;
    });
}

async function finishInstall(gameId, gameName, mountPoint) {
  const el = body();
  const info = state.drmCache[gameId];
  const easy = info && info.preservability && info.preservability.kind === "easy";

  // Best effort, no API key configured or no art on SteamGridDB is not a
  // failure of the install — the launcher (#204) just shows no cover art
  // for this entry.
  invoke("fetch_cartridge_art", { appId: gameId, mountPoint }).catch(() => {});
  invoke("fetch_cartridge_description", { appId: gameId, mountPoint }).catch(() => {});

  if (!easy) {
    el.innerHTML = `<div class="import-result import-result-ok">✓ "${esc(gameName)}" instalado en el cartucho. Jugable desde Steam.</div>`;
    return;
  }

  el.innerHTML = `<div class="loading"><div class="spinner"></div><br>Preparando modo standalone (Goldberg)...</div>`;
  try {
    await invoke("inject_goldberg", { appId: gameId, mountPoint });
  } catch (e) {
    // The install itself already succeeded — a failed Goldberg step (e.g. a
    // SteamStub wrapper #199 doesn't unpack) is a warning, not a failure of
    // this flow: the game still plays fine through Steam.
    el.innerHTML =
      `<div class="import-result import-result-ok">✓ "${esc(gameName)}" instalado, jugable desde Steam.</div>` +
      `<div class="cartridge-warn">No se pudo preparar el modo standalone: ${esc(String(e))}</div>`;
    return;
  }

  // Goldberg alone isn't enough to run standalone on Linux — the launcher
  // (#204) also needs umu-run + Proton + its Steam Linux Runtime bundled
  // on the cartridge (#206). Only the first "Easy" game on a cartridge
  // actually triggers a download; every one after just copies the cache.
  el.innerHTML = `<div class="loading"><div class="spinner"></div><br>Preparando runtime de Linux (Proton)...</div>`;
  try {
    await invoke("bundle_linux_runtime", { mountPoint });
    el.innerHTML = `<div class="import-result import-result-ok">✓ "${esc(gameName)}" instalado y jugable standalone (sin Steam) vía Proton.</div>`;
  } catch (e) {
    el.innerHTML =
      `<div class="import-result import-result-ok">✓ "${esc(gameName)}" instalado, jugable desde Steam.</div>` +
      `<div class="cartridge-warn">No se pudo preparar el runtime de Linux: ${esc(String(e))}</div>`;
  }
}
