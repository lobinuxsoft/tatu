import { invoke, getVersion, listen } from "./js/tauri.js";
import { state } from "./js/state.js";
import { renderSteam } from "./js/render/steam.js";
import { renderNonSteam } from "./js/render/nonsteam.js";
import { installExternalLinks } from "./js/links.js";
import { openImportModal, closeImportModal } from "./js/modals/import.js";
import { doSync, doSyncNonSteam, doScanSizes, doFetchAllDrm } from "./js/actions.js";
import { loadSettingsUI, checkConfigWarning, installSettingsHandlers } from "./js/settings.js";
import { initTheme, installThemeSwitcher } from "./js/themes.js";
import { openCartridgeManagePanel } from "./js/panel/cartridge_manage.js";

initTheme();

async function init() {
  try {
    state.cheatsSupported = await invoke("cheats_supported");
  } catch (_) {
    state.cheatsSupported = false;
  }

  // Before get_state: if the library fails to load, the help panel is exactly
  // where the user goes to find out what they were supposed to configure.
  fillHelpPanel();

  try {
    const data = await invoke("get_state");
    state.G = data.games || [];
    state.completed = new Set(data.completed || []);
    state.achProgress = data.ach_progress || {};
    state.hltbCache = data.hltb_cache || {};
    state.drmCache = data.drm_cache || {};
    state.sizeCache = data.size_cache || {};
    state.NS = data.non_steam || [];
    state.completedNS = new Set(data.completed_nonsteam || []);

    loadSettingsUI(data.steam_api_key, data.steam_id, data.steamgriddb_api_key);

    if (!data.steam_id) {
      try {
        const detectedId = await invoke("detect_steam_id");
        if (detectedId) document.getElementById("cfgSteamId").value = detectedId;
      } catch (_) {}
    }
    checkConfigWarning();

    try {
      const favIds = await invoke("get_steam_favorites");
      state.favorites = new Set(favIds || []);
    } catch (_) {
      state.favorites = new Set();
    }

    document.getElementById("subtitle").textContent = state.G.length + " juegos Steam + " + state.NS.length + " Non-Steam";
    if (state.G.length > 0) renderSteam();
    else if (state.hasConfig) await doSync();
    renderNonSteam();
  } catch (e) {
    document.getElementById("content").innerHTML = '<div class="loading" style="color:#f85149">Error: ' + e + '</div>';
  }
}

// The help panel states two things this build cannot know at author time:
// whether cheats exist on this platform, and where state.json really lives.
async function fillHelpPanel() {
  const cheats = document.getElementById("helpCheats");
  if (cheats) {
    cheats.innerHTML = state.cheatsSupported
      ? 'Cada juego tiene una pestaña <strong>Cheats</strong> donde podés importar una tabla ' +
        '<code>.CT</code> de Cheat Engine y activar sus opciones. Sólo aplica sobre un proceso ' +
        'que arrancaste vos, y los juegos con anti-cheat quedan fuera a propósito.'
      : 'No disponibles en esta plataforma. El motor de cheats corre sobre <code>ptrace</code>, ' +
        'que no existe en Windows — por eso la pestaña no aparece. Está en camino un backend ' +
        'nativo de Windows.';
  }

  try {
    const path = await invoke("state_path");
    for (const id of ["helpStatePath", "settingsStatePath"]) {
      const el = document.getElementById(id);
      if (el) el.textContent = path;
    }
    const footer = document.getElementById("footerStatePath");
    if (footer) footer.textContent = "Estado guardado en " + path + " \u2014 ";
  } catch (_) {
    // Leave the placeholders rather than showing a wrong path.
  }
}

// --- Tabs ---
document.querySelector(".tabs").addEventListener("click", e => {
  const tab = e.target.closest(".tab"); if (!tab) return;
  document.querySelectorAll(".tab").forEach(t => t.classList.remove("active"));
  document.querySelectorAll(".tab-panel").forEach(p => p.classList.remove("active"));
  tab.classList.add("active");
  document.getElementById("panel-" + tab.dataset.tab).classList.add("active");
  if (tab.dataset.tab === "cartridge") openCartridgeManagePanel();
});

// --- Sync / action buttons ---
document.getElementById("syncBtn").addEventListener("click", () => {
  if (!state.hasConfig) {
    document.getElementById("syncInfo").textContent = "Configurá API Key y Steam ID en Settings primero.";
    return;
  }
  doSync();
});
document.getElementById("nsSyncBtn").addEventListener("click", doSyncNonSteam);
document.getElementById("drmBtn").addEventListener("click", doFetchAllDrm);
document.getElementById("sizeBtn").addEventListener("click", doScanSizes);
document.getElementById("importBtn").addEventListener("click", openImportModal);
document.getElementById("importClose").addEventListener("click", closeImportModal);
document.getElementById("importOverlay").addEventListener("click", e => {
  if (e.target.id === "importOverlay") closeImportModal();
});

// --- Filter / sort rows ---
document.getElementById("catRow").addEventListener("click", e => {
  const b = e.target.closest(".tbtn"); if (!b) return;
  state.tog[b.dataset.t] = !state.tog[b.dataset.t];
  renderSteam();
});
document.getElementById("statusRow").addEventListener("click", e => {
  const b = e.target.closest(".sbtn"); if (!b) return;
  document.querySelectorAll("#statusRow .sbtn").forEach(x => x.classList.remove("active"));
  b.classList.add("active"); state.sf = b.dataset.s; renderSteam();
});
document.getElementById("sortRow").addEventListener("click", e => {
  const b = e.target.closest(".sbtn"); if (!b) return;
  state.sortMode = b.dataset.sort; renderSteam();
});
document.getElementById("presRow").addEventListener("click", e => {
  const b = e.target.closest(".pbtn"); if (!b) return;
  document.querySelectorAll("#presRow .pbtn").forEach(x => x.classList.remove("active"));
  b.classList.add("active"); state.pf = b.dataset.p; renderSteam();
});

// --- Completed checkboxes ---
document.addEventListener("change", async e => {
  if (e.target.type === "checkbox" && e.target.dataset.id) {
    const id = parseInt(e.target.dataset.id, 10);
    const list = e.target.dataset.list;
    if (list === "steam") {
      if (e.target.checked) state.completed.add(id); else state.completed.delete(id);
      renderSteam();
      await invoke("save_completed", { completed: [...state.completed] });
    } else if (list === "nonsteam") {
      if (e.target.checked) state.completedNS.add(id); else state.completedNS.delete(id);
      renderNonSteam();
      await invoke("save_completed_nonsteam", { completed: [...state.completedNS] });
    }
  }
});

// --- Search + letter nav + row click ---
document.getElementById("search").addEventListener("input", e => {
  state.q = e.target.value.toLowerCase().trim();
  renderSteam();
});
document.getElementById("lNav").addEventListener("click", e => {
  if (e.target.tagName === "A") {
    e.preventDefault();
    const t = document.querySelector(e.target.getAttribute("href"));
    if (t) t.scrollIntoView({ behavior: "smooth", block: "start" });
  }
});
document.getElementById("content").addEventListener("click", e => {
  if (e.target.type === "checkbox") return;
  const tr = e.target.closest("tr");
  if (!tr) return;
  const cb = tr.querySelector("input[data-id][data-list='steam']");
  if (!cb) return;
  // #187: the detail view is its own window, so it can be moved, resized past
  // this window's bounds, and left open beside the list.
  invoke("open_detail_window", { appId: parseInt(cb.dataset.id, 10) })
    .catch(e => console.error("open_detail_window failed", e));
});

// The detail window fills the DRM / HowLongToBeat / achievement caches for
// whichever game is open there. Re-read state so the list behind it reflects
// what was just fetched instead of going stale until the next sync.
listen("library-updated", async () => {
  try {
    const data = await invoke("get_state");
    state.G = data.games || [];
    state.achProgress = data.ach_progress || {};
    state.hltbCache = data.hltb_cache || {};
    state.drmCache = data.drm_cache || {};
    state.sizeCache = data.size_cache || {};
    renderSteam();
  } catch (_) {
    // A failed refresh just means the list is a sync behind; not worth a toast.
  }
});

installSettingsHandlers();
installThemeSwitcher();
installExternalLinks();

getVersion().then(v => { document.getElementById("appVersion").textContent = "v" + v; });
init();
