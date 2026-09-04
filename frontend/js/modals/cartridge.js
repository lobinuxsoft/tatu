import { invoke } from "../tauri.js";
import { state } from "../state.js";
import { esc, formatBytes } from "../utils.js";
import { renderFormatConfirm } from "./format_cartridge.js";

// Guards against a leaked timer if the modal is closed mid-poll or a second
// install is started on top of one already running.
let pollTimer = null;

export async function openCartridgeModal(gameId) {
  const game = state.G.find(g => g.id === gameId);
  if (!game) return;
  document.getElementById("cartridgeOverlay").classList.remove("hidden");

  // A manifest already on a connected cartridge is real evidence an install
  // started there, regardless of whether this modal (or Tatu itself) was
  // open the whole time — resume watching it directly instead of making
  // the user re-pick the drive and re-click install.
  const el = body();
  el.innerHTML = `<div class="loading"><div class="spinner"></div><br>Buscando discos...</div>`;
  try {
    const pending = await invoke("find_pending_cartridge", { appId: gameId });
    if (pending) {
      showInstall(gameId, game.name, pending);
      return;
    }
  } catch {
    // Fall through to the normal picker — this is a best-effort shortcut.
  }
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
      // (steamapps/ needs write access), so it's a dead end either way. An
      // unmounted drive isn't a dead end though — clicking it mounts it.
      const disabled = drive.read_only ? " disabled" : "";
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

    el.onclick = async e => {
      const row = e.target.closest(".collection-row");
      if (!row || row.classList.contains("disabled")) return;
      const picked = rows.find(r => r.drive.id === row.dataset.id);
      if (!picked) return;

      if (!picked.drive.mount_point) {
        el.innerHTML = `<div class="loading"><div class="spinner"></div><br>Montando disco...</div>`;
        try {
          const mountPoint = await invoke("mount_cartridge", { device: picked.drive.id });
          const alreadyCartridge = await invoke("has_cartridge_structure", { mountPoint });
          if (alreadyCartridge) {
            showRegistrationCheck(gameId, gameName, mountPoint);
          } else {
            showFormatConfirm(gameId, gameName, { ...picked.drive, mount_point: mountPoint });
          }
        } catch (err) {
          el.innerHTML =
            `<div class="cartridge-warn">No se pudo montar: ${esc(String(err))}</div>` +
            `<div class="cartridge-actions"><button class="cartridge-btn-secondary" id="cartMountBack">Volver</button></div>`;
          document.getElementById("cartMountBack").onclick = () => showDriveList(gameId, gameName);
        }
        return;
      }

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
  renderFormatConfirm(el, drive, {
    onCancel: () => showDriveList(gameId, gameName),
    onSuccess: fresh => showRegistrationCheck(gameId, gameName, fresh.mount_point),
    onError: msg => {
      el.innerHTML =
        `<div class="cartridge-warn">${esc(msg)}</div>` +
        `<div class="cartridge-actions"><button class="cartridge-btn-secondary" id="cartFormatBack">Volver</button></div>`;
      document.getElementById("cartFormatBack").onclick = () => showDriveList(gameId, gameName);
    },
  });
}

async function showRegistrationCheck(gameId, gameName, mountPoint) {
  const el = body();
  el.innerHTML = `<div class="loading"><div class="spinner"></div><br>Verificando registro en Steam...</div>`;
  try {
    const registered = await invoke("is_registered_library", { mountPoint });
    if (registered) {
      showVersionChoice(gameId, gameName, mountPoint);
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

// A cartridge has to work when plugged into another machine's Steam client
// or the bundled launcher's Proton — Steam's own native Linux build never
// satisfies either (live case, 2026-08-29: CrossCode installed native,
// Goldberg's own steam_api swap never had a .exe to mark standalone
// against). Windows/Proton is the default every time, but still asked
// per-game rather than forced silently, per the user's own "por las
// dudas" — some game might have a real reason to want the native build.
function showVersionChoice(gameId, gameName, mountPoint) {
  const el = body();
  el.innerHTML =
    `<div class="cartridge-guide">¿Qué versión instalar?</div>` +
    `<div class="cartridge-actions">` +
    `<button class="cartridge-btn" id="cartVerProton">Windows / Proton (recomendado)</button>` +
    `<button class="cartridge-btn-secondary" id="cartVerNative">Nativa de este sistema</button>` +
    `</div>`;
  document.getElementById("cartVerProton").onclick = () =>
    applyProtonAndInstall(gameId, gameName, mountPoint);
  document.getElementById("cartVerNative").onclick = () => showInstall(gameId, gameName, mountPoint);
}

async function applyProtonAndInstall(gameId, gameName, mountPoint) {
  const el = body();
  el.innerHTML =
    `<div class="loading"><div class="spinner"></div><br>Cerrando Steam para forzar la versión de ` +
    `Windows (reescribe su configuración al abrir, así que tiene que estar cerrado un momento) — ` +
    `se vuelve a abrir solo al instalar...</div>`;
  try {
    await invoke("force_proton_compat", { appId: gameId });
    showInstall(gameId, gameName, mountPoint);
  } catch (e) {
    const raw = String(e);
    // force_proton_compat now closes Steam itself — this only fires if
    // that genuinely didn't work (Steam ignored the close, or took longer
    // than the wait), not the normal "please close it" case anymore.
    const steamOpen = /steam is running/i.test(raw);
    el.innerHTML =
      `<div class="cartridge-warn">${steamOpen ? "No pude cerrar Steam solo — cerralo a mano y reintentá." : esc(raw)}</div>` +
      `<div class="cartridge-actions">` +
      `<button class="cartridge-btn-secondary" id="cartVerBack">Volver</button>` +
      `<button class="cartridge-btn" id="cartVerRetry">Reintentar</button>` +
      `</div>`;
    document.getElementById("cartVerBack").onclick = () => showVersionChoice(gameId, gameName, mountPoint);
    document.getElementById("cartVerRetry").onclick = () =>
      applyProtonAndInstall(gameId, gameName, mountPoint);
  }
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

// Art, description and the Linux runtime are shared by the whole cartridge,
// not tied to this one install — they're batched into the "Cartucho" tab's
// "Preparar launcher" step instead of firing here per game. This only does
// what genuinely can't wait: patching THIS game's just-downloaded files.
const CARTRIDGE_TAB_HINT =
  ' Andá a la pestaña "Cartucho" para preparar el launcher cuando termines de instalar juegos.';

// Live-tested (2026-08-28): Steam bajando/commiteando el shader cache
// directo sobre un pendrive USB tardó ~10 minutos y dejó el cliente con el
// hilo principal trabado (assert "BMainLoop stalled") — se ve exactamente
// como un cuelgue aunque termina solo. Pasa la primera vez que Steam corre
// este juego en esta máquina/prefix, sea desde acá o desde el launcher.
const FIRST_LAUNCH_HINT =
  ' El primer lanzamiento por Steam puede tardar varios minutos (shader cache) y Steam puede parecer ' +
  'congelado mientras tanto — es normal en un pendrive, no lo cierres.';

async function finishInstall(gameId, gameName, mountPoint) {
  const el = body();
  const info = state.drmCache[gameId];
  const easy = info && info.preservability && info.preservability.kind === "easy";

  if (!easy) {
    el.innerHTML = `<div class="import-result import-result-ok">✓ "${esc(gameName)}" instalado en el cartucho. Jugable desde Steam.${FIRST_LAUNCH_HINT}${CARTRIDGE_TAB_HINT}</div>`;
    return;
  }

  el.innerHTML = `<div class="loading"><div class="spinner"></div><br>Preparando modo standalone (Goldberg)...</div>`;
  try {
    await invoke("inject_goldberg", { appId: gameId, mountPoint });
    el.innerHTML = `<div class="import-result import-result-ok">✓ "${esc(gameName)}" instalado, listo para standalone.${CARTRIDGE_TAB_HINT}</div>`;
  } catch (e) {
    // The install itself already succeeded — a failed Goldberg step (e.g. a
    // SteamStub wrapper #199 doesn't unpack) is a warning, not a failure of
    // this flow: the game still plays fine through Steam.
    el.innerHTML =
      `<div class="import-result import-result-ok">✓ "${esc(gameName)}" instalado, jugable desde Steam.</div>` +
      `<div class="cartridge-warn">No se pudo preparar el modo standalone: ${esc(String(e))}</div>`;
  }
}
