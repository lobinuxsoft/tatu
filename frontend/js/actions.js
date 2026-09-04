import { invoke, listen } from "./tauri.js";
import { state } from "./state.js";
import { formatBytes } from "./utils.js";
import { renderSteam } from "./render/steam.js";
import { renderNonSteam } from "./render/nonsteam.js";
import { renderGog } from "./render/gog.js";

export async function doSync() {
  const btn = document.getElementById("syncBtn");
  btn.disabled = true; btn.textContent = "Sincronizando...";
  try {
    state.G = await invoke("sync_steam");
    document.getElementById("subtitle").textContent = state.G.length + " juegos Steam + " + state.NS.length + " Non-Steam";
    document.getElementById("syncInfo").textContent = `Sincronizado \u2014 ${state.G.length} juegos`;
    renderSteam();
  } catch (e) {
    document.getElementById("syncInfo").textContent = "Error: " + e;
  } finally {
    btn.disabled = false; btn.textContent = "Sincronizar";
  }
}

export async function doSyncNonSteam() {
  const btn = document.getElementById("nsSyncBtn");
  btn.disabled = true; btn.textContent = "Leyendo...";
  try {
    state.NS = await invoke("sync_nonsteam");
    document.getElementById("nsSyncInfo").textContent = `${state.NS.length} juegos Non-Steam encontrados`;
    renderNonSteam();
  } catch (e) {
    document.getElementById("nsSyncInfo").textContent = "Error: " + e;
  } finally {
    btn.disabled = false; btn.textContent = "Leer shortcuts.vdf";
  }
}

export async function doScanSizes() {
  const btn = document.getElementById("sizeBtn");
  const info = document.getElementById("syncInfo");
  btn.disabled = true; btn.textContent = "Escaneando...";
  try {
    const scanned = await invoke("scan_sizes");
    for (const s of scanned) { state.sizeCache[s.app_id] = s; }
    const installedCount = scanned.filter(s => s.source && s.source.kind === "local_manifest").length;
    const appinfoCount = scanned.filter(s => s.source && s.source.kind === "appinfo").length;
    const totalBytes = scanned.reduce((sum, s) => sum + (s.bytes || 0), 0);
    info.textContent = `Escaneo: ${installedCount} instalados + ${appinfoCount} vía appinfo.vdf (upper bound). Total: ${formatBytes(totalBytes)}.`;
    renderSteam();
  } catch (e) {
    info.textContent = "Error al escanear tamaño: " + e;
  } finally {
    btn.disabled = false; btn.textContent = "Escanear tamaño";
  }
}

export async function doFetchAllDrm() {
  const btn = document.getElementById("drmBtn");
  const info = document.getElementById("syncInfo");
  const wrap = document.getElementById("drmProgressWrap");
  const bar = document.getElementById("drmProgressBar");
  const text = document.getElementById("drmProgressText");
  btn.disabled = true; btn.textContent = "Cargando DRM...";
  const prevInfo = info.textContent;

  // A small text badge next to the sync buttons was too easy to miss
  // against 500+ games loading below it — a real progress bar, shown only
  // while this runs, reuses the same visual language the completion bar
  // above already has (#234).
  wrap.style.display = "";
  bar.style.width = "0%";
  text.textContent = "DRM Análisis 0/0";

  const unlisten = await listen("drm_progress", e => {
    const p = e.payload || {};
    bar.style.width = `${(p.current / p.total) * 100}%`;
    text.textContent = `DRM Análisis ${p.current}/${p.total}`;
    if (p.app_id && p.info) {
      state.drmCache[p.app_id] = p.info;
      renderSteam();
    }
  });
  const unlistenDone = await listen("drm_done", e => {
    const p = e.payload || {};
    info.textContent = prevInfo || `DRM cargado para ${p.total} juegos`;
    wrap.style.display = "none";
    btn.disabled = false; btn.textContent = "Cargar DRM";
    unlisten(); unlistenDone();
  });

  try {
    await invoke("fetch_all_drm");
  } catch (e) {
    info.textContent = "Error DRM: " + e;
    wrap.style.display = "none";
    btn.disabled = false; btn.textContent = "Cargar DRM";
    unlisten(); unlistenDone();
  }
}

// Bulk companion to detail_cmd's per-game lazy fetch (`get_game_details`
// only fills genres/developers/publishers for a game once its own detail
// panel is opened) — needed for the search box to actually match those
// fields across a library nobody has clicked through row by row.
export async function doFetchAllDetails() {
  const btn = document.getElementById("detailsBtn");
  const info = document.getElementById("syncInfo");
  const wrap = document.getElementById("detailsProgressWrap");
  const bar = document.getElementById("detailsProgressBar");
  const text = document.getElementById("detailsProgressText");
  btn.disabled = true; btn.textContent = "Cargando Detalles...";
  const prevInfo = info.textContent;

  wrap.style.display = "";
  bar.style.width = "0%";
  text.textContent = "Detalles 0/0";

  const unlisten = await listen("detail_progress", e => {
    const p = e.payload || {};
    bar.style.width = `${(p.current / p.total) * 100}%`;
    text.textContent = `Detalles ${p.current}/${p.total}`;
  });
  const unlistenDone = await listen("details_done", e => {
    const p = e.payload || {};
    if (p.games) { state.G = p.games; renderSteam(); }
    info.textContent = prevInfo || "Detalles cargados";
    wrap.style.display = "none";
    btn.disabled = false; btn.textContent = "Cargar Detalles";
    unlisten(); unlistenDone();
  });

  try {
    await invoke("fetch_details");
  } catch (e) {
    info.textContent = "Error al cargar detalles: " + e;
    wrap.style.display = "none";
    btn.disabled = false; btn.textContent = "Cargar Detalles";
    unlisten(); unlistenDone();
  }
}

// One request per owned title (#243) — same reasoning doFetchAllDrm has for
// its own progress bar: silently blocking a tab for however long a real
// library takes to resolve looks exactly like a hang.
export async function doFetchGogLibrary() {
  const btn = document.getElementById("gogTabSyncBtn");
  const info = document.getElementById("gogSyncInfo");
  btn.disabled = true; btn.textContent = "Actualizando...";
  const games = [];

  const unlistenProgress = await listen("gog_library_progress", e => {
    const p = e.payload || {};
    info.textContent = `Resolviendo biblioteca de GOG: ${p.current}/${p.total}...`;
    if (p.game) games.push(p.game);
  });
  const unlistenDone = await listen("gog_library_done", e => {
    const p = e.payload || {};
    state.GOG = games;
    renderGog();
    info.textContent = `Biblioteca de GOG actualizada — ${p.total} juegos`;
    btn.disabled = false; btn.textContent = "Actualizar biblioteca";
    unlistenProgress(); unlistenDone(); unlistenError();
  });
  const unlistenError = await listen("gog_library_error", e => {
    info.textContent = "Error: " + e.payload;
    btn.disabled = false; btn.textContent = "Actualizar biblioteca";
    unlistenProgress(); unlistenDone(); unlistenError();
  });

  try {
    await invoke("fetch_gog_library");
  } catch (e) {
    info.textContent = "Error: " + e;
    btn.disabled = false; btn.textContent = "Actualizar biblioteca";
    unlistenProgress(); unlistenDone(); unlistenError();
  }
}
