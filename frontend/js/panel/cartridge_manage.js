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
    // Best-effort (#228): the app list is the important part, a usage scan
    // failing (a mid-eject race, an odd filesystem) shouldn't block it.
    const usage = await invoke("get_cartridge_usage", { mountPoint: drive.mount_point }).catch(() => null);
    renderCartridge(drive, apps, usage);
  } catch (e) {
    el.innerHTML = `<div class="cartridge-warn">Error al leer el cartucho: ${esc(String(e))}</div>`;
  }
}

// Stable per-game color: hashed from the app_id rather than array index, so
// a game keeps the same color across re-renders even if others are
// added/removed around it. The launcher and "Otros" get fixed colors —
// always present, always meant to stand out/recede the same way.
function colorForApp(appId) {
  const hue = (appId * 47) % 360;
  return `hsl(${hue}, 60%, 55%)`;
}

// Bar + legend (#228): total/free/used, with used broken into the launcher
// (ONE combined segment — the player doesn't care it's a binary plus a
// bundled runtime), one segment per game, and a residual "Otros" so the
// bar always adds up to what the filesystem itself reports as used.
function renderUsage(usage) {
  if (!usage) return "";

  const segments = [{ label: "Launcher", bytes: usage.launcher_bytes, color: "var(--accent)" }];
  for (const app of usage.apps) {
    segments.push({ label: app.name, bytes: app.bytes, color: colorForApp(app.app_id) });
  }
  if (usage.other_bytes > 0) {
    segments.push({ label: "Otros", bytes: usage.other_bytes, color: "var(--fg-dim)" });
  }

  const total = usage.total_bytes || 1;
  const bar = segments
    .map(
      s =>
        `<div class="cartridge-usage-segment" style="width:${(s.bytes / total) * 100}%; background:${s.color}" title="${esc(s.label)}: ${formatBytes(s.bytes)}"></div>`
    )
    .join("");
  const freeBar = `<div class="cartridge-usage-segment is-free" style="width:${(usage.free_bytes / total) * 100}%" title="Libre: ${formatBytes(usage.free_bytes)}"></div>`;

  const legend = segments
    .concat([{ label: "Libre", bytes: usage.free_bytes, color: null }])
    .map(
      s =>
        `<div class="cartridge-usage-legend-item">` +
        `<span class="cartridge-usage-swatch${s.color ? "" : " is-free"}" style="${s.color ? `background:${s.color}` : ""}"></span>` +
        `${esc(s.label)} — ${formatBytes(s.bytes)}</div>`
    )
    .join("");

  return (
    `<div class="cartridge-usage">` +
    `<div class="cartridge-usage-summary">${formatBytes(usage.free_bytes)} libres de ${formatBytes(usage.total_bytes)}</div>` +
    `<div class="cartridge-usage-bar">${bar}${freeBar}</div>` +
    `<div class="cartridge-usage-legend">${legend}</div>` +
    `</div>`
  );
}

function renderCartridge(drive, apps, usage) {
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
    renderUsage(usage) +
    rows +
    // Opt-in (#212): a trailer runs 10-100+ MB and takes real time to
    // transcode, unlike art/description below which are always-on and
    // basically free — never checked by default.
    `<label class="cartridge-guide" style="display:flex; align-items:center; gap:8px; cursor:pointer;">` +
    `<input type="checkbox" id="cartIncludeTrailers"> Incluir trailers (pesa más, tarda más por juego)</label>` +
    `<div class="cartridge-actions">` +
    `<button class="cartridge-btn-secondary" id="cartManageBack">Volver a discos</button>` +
    `<button class="cartridge-btn" id="cartPrepareBtn">Preparar launcher</button>` +
    `</div>` +
    `<div id="cartPrepareResult"></div>`;

  document.getElementById("cartManageBack").onclick = () => showDriveList();
  document.getElementById("cartPrepareBtn").onclick = () =>
    prepareLauncher(drive, apps, document.getElementById("cartIncludeTrailers").checked);
}

// Everything that's shared by the WHOLE cartridge rather than tied to one
// game's install — the launcher binary, the Linux runtime, and every app's
// art/description — batched here instead of firing per-game at install
// time (finishInstall in cartridge.js only does Goldberg injection now).
async function prepareLauncher(drive, apps, includeTrailers) {
  const result = document.getElementById("cartPrepareResult");
  const mountPoint = drive.mount_point;
  const setStatus = msg => {
    result.innerHTML = `<div class="loading"><div class="spinner"></div><br>${esc(msg)}</div>`;
  };

  // Found live-testing with the user: nothing stopped "Volver a discos" or
  // a second "Preparar launcher" click mid-run — either tears down the DOM
  // nodes this function is still writing to, or races a second copy of the
  // same invokes. Locked for the duration, restored in `finally` either way.
  const checkbox = document.getElementById("cartIncludeTrailers");
  const backBtn = document.getElementById("cartManageBack");
  const prepareBtn = document.getElementById("cartPrepareBtn");
  checkbox.disabled = true;
  backBtn.disabled = true;
  prepareBtn.disabled = true;

  try {
    setStatus("Copiando el launcher (Linux + Windows)...");
    await invoke("install_launcher_binaries", { mountPoint });

    if (apps.some(a => a.preservability && a.preservability.kind === "easy")) {
      // Hundreds of MB copied from Tatu's own cache onto whatever drive is
      // plugged in — no network involved, but real disk write time on a
      // slow USB stick, same one #217 already found stalling under load.
      setStatus("Preparando runtime de Linux (Proton)... puede tardar según la velocidad del disco.");
      await invoke("bundle_linux_runtime", { mountPoint });
    }

    for (let i = 0; i < apps.length; i++) {
      const app = apps[i];
      setStatus(`Bajando arte, descripción y capturas (${i + 1}/${apps.length}): ${app.name}`);
      await Promise.all([
        invoke("fetch_cartridge_art", { appId: app.app_id, mountPoint }).catch(() => {}),
        invoke("fetch_cartridge_description", { appId: app.app_id, mountPoint }).catch(() => {}),
        invoke("fetch_cartridge_screenshots", { appId: app.app_id, mountPoint }).catch(() => {}),
      ]);
    }

    // Separate loop, after art/description: transcoding a trailer takes
    // real wall-clock time (ffmpeg, not just an HTTP GET), worth its own
    // per-game progress line instead of hiding inside the loop above.
    if (includeTrailers) {
      for (let i = 0; i < apps.length; i++) {
        const app = apps[i];
        setStatus(`Bajando trailer (${i + 1}/${apps.length}): ${app.name}...`);
        await invoke("fetch_cartridge_trailer", { appId: app.app_id, mountPoint }).catch(() => {});
      }
    }

    result.innerHTML =
      `<div class="import-result import-result-ok">✓ Cartucho listo — launcher instalado ` +
      `para Linux y Windows, ${apps.length} juego(s) preparados.</div>`;

    // Space just moved a lot (runtime bundled, art/screenshots/trailers
    // downloaded) — the bar chart drawn before this run started is stale.
    const usageEl = document.querySelector(".cartridge-usage");
    if (usageEl) {
      const usage = await invoke("get_cartridge_usage", { mountPoint }).catch(() => null);
      usageEl.outerHTML = renderUsage(usage);
    }
  } catch (e) {
    result.innerHTML = `<div class="cartridge-warn">No se pudo terminar de preparar el cartucho: ${esc(String(e))}</div>`;
  } finally {
    checkbox.disabled = false;
    backBtn.disabled = false;
    prepareBtn.disabled = false;
  }
}
