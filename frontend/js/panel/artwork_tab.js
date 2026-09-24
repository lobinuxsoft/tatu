import { invoke } from "../tauri.js";
import { esc } from "../utils.js";

// Manual SteamGridDB art picker (#328) — ported UX from capydeploy hub's
// own `ArtworkSelector.svelte` (same author, same API, deliberately the
// same 5-slot/5-tab shape), rewritten in vanilla JS (no Svelte/Vite
// pipeline here) as a SECOND TAB on the non-Steam detail window rather
// than a modal — live feedback: a floating picker on top of the ficha
// broke the "same window template as Steam/GOG" goal #328 started from;
// Steam's own Info/Logros/Cromos/Cheats are already tabs, this fits the
// same slot. `grids` covers both "capsule" (portrait) and "wide"
// (landscape) — one endpoint, split client-side by aspect ratio.
const TABS = {
  capsule: { label: "Capsule", slot: "grid_portrait", fetch: "steamgriddb_grids", filter: img => img.height > img.width },
  wide: { label: "Wide", slot: "grid_landscape", fetch: "steamgriddb_grids", filter: img => img.width > img.height },
  hero: { label: "Hero", slot: "hero", fetch: "steamgriddb_heroes" },
  logo: { label: "Logo", slot: "logo", fetch: "steamgriddb_logos" },
  icon: { label: "Icon", slot: "icon", fetch: "steamgriddb_icons" },
};

let targetId = null; // Steam appid, GOG product id, or Non-Steam shortcut id
let gameName = "";
let selection = emptySelection();
let activeTab = "capsule";
let imageCache = {}; // tab key -> ArtImage[]
let onSaved = null;
let searched = false; // lazy: only hits SteamGridDB once the tab is actually opened
let imageType = ""; // "" (both) | "static" | "animated" — SteamGridDB's own `types` filter
let appliedAnimated = true;
let appliedStatic = true;
let appliedNsfw = false;
let appliedHumor = true;

function emptySelection() {
  return { griddb_game_id: 0, grid_portrait: "", grid_landscape: "", hero: "", logo: "", icon: "" };
}

function toggleHtml(id, label, checked = true) {
  return `<label class="art-toggle"><input type="checkbox" id="${id}"${checked ? " checked" : ""}><span class="art-toggle-track"></span>${esc(label)}</label>`;
}

// Selected row + Guardar/Limpiar sit ABOVE the results grid, not below —
// live feedback: with 380 results on a single tab (DOOM's own grid count),
// "which ones did I pick" was off-screen at the bottom of a very long page.
// The grid itself scrolls in its own fixed-height box instead of growing
// the whole tab; the picks and the buttons for them never move.
export function artworkTabHtml() {
  return (
    `<div class="art-search-row">` +
    `<input type="text" id="artworkSearchInput" placeholder="Buscar juego en SteamGridDB...">` +
    `<button class="cartridge-btn-secondary" id="artworkSearchBtn">Buscar</button>` +
    `</div>` +
    `<div id="artworkSearchResults" class="art-search-results"></div>` +
    `<div id="artworkSelectedRow" class="art-selected-row"></div>` +
    `<div class="cartridge-actions">` +
    `<button class="cartridge-btn-secondary" id="artworkClear">Limpiar todo</button>` +
    `<button class="cartridge-btn" id="artworkSave">Guardar</button>` +
    `</div>` +
    `<div class="art-tabs-row">` +
    `<div class="art-tabs" id="artworkTabs">` +
    Object.entries(TABS).map(([key, t]) => `<div class="art-tab${key === "capsule" ? " active" : ""}" data-art="${key}">${esc(t.label)}</div>`).join("") +
    `</div>` +
    `<button class="cartridge-btn-secondary" id="artworkFiltersBtn">▽ Filtros</button>` +
    `</div>` +
    `<div class="art-filters-panel hidden" id="artworkFiltersPanel">` +
    `<div class="art-filters-label">Tipos</div>` +
    `<div class="art-filters-row">` +
    toggleHtml("artworkShowAnimated", "Animados") +
    toggleHtml("artworkShowStatic", "Estáticos") +
    `</div>` +
    `<div class="art-filters-label">Etiquetas</div>` +
    `<div class="art-filters-row">` +
    toggleHtml("artworkShowNsfw", "Contenido adulto", false) +
    toggleHtml("artworkShowHumor", "Humor", true) +
    `</div>` +
    `<div class="cartridge-actions">` +
    `<button class="cartridge-btn-secondary" id="artworkFiltersCancel">Cancelar</button>` +
    `<button class="cartridge-btn" id="artworkFiltersApply">Aplicar</button>` +
    `</div>` +
    `</div>` +
    `<div class="art-grid-scroll">` +
    `<div id="artworkGrid" class="art-grid"></div>` +
    `<div id="artworkLoadMoreRow"></div>` +
    `</div>`
  );
}

/// Wires the tab's own internal handlers. Called once, right after
/// `detailTabsShell`'s HTML (which embeds `artworkTabHtml()` as one panel)
/// is in the DOM. `id` is whatever id `state.artwork` is keyed by for this
/// source (Steam appid / GOG product id / Non-Steam shortcut id) — same
/// tab works for all three (#328). `onSave(selection)` is optional, for a
/// caller that wants to refresh its own inline art preview without a full
/// detail re-render.
export function installArtworkTabHandlers(id, name, onSave) {
  targetId = id;
  gameName = name;
  selection = emptySelection();
  activeTab = "capsule";
  imageCache = {};
  onSaved = onSave;
  searched = false;
  imageType = "";
  appliedAnimated = true;
  appliedStatic = true;
  appliedNsfw = false;
  appliedHumor = true;

  document.getElementById("artworkSearchInput").value = gameName;
  installFiltersPanel();
  renderSelectedRow();

  document.getElementById("artworkSearchBtn").onclick = () => doSearch(document.getElementById("artworkSearchInput").value);
  document.getElementById("artworkSearchInput").addEventListener("keydown", e => {
    if (e.key === "Enter") doSearch(e.target.value);
  });
  document.getElementById("artworkTabs").addEventListener("click", e => {
    const tab = e.target.closest(".art-tab");
    if (!tab) return;
    activeTab = tab.dataset.art;
    document.querySelectorAll(".art-tab").forEach(t => t.classList.toggle("active", t === tab));
    loadTab(activeTab, false);
  });
  document.getElementById("artworkClear").onclick = () => {
    selection = { ...emptySelection(), griddb_game_id: selection.griddb_game_id };
    renderSelectedRow();
    renderGrid();
  };
  document.getElementById("artworkSave").onclick = async () => {
    const btn = document.getElementById("artworkSave");
    btn.disabled = true;
    try {
      await invoke("set_artwork", { id: targetId, artwork: selection });
      if (onSaved) onSaved({ ...selection });
    } catch (e) {
      document.getElementById("artworkSaveErr")?.remove();
      document.getElementById("artworkSelectedRow").insertAdjacentHTML(
        "afterend",
        `<div class="cartridge-warn" id="artworkSaveErr">No pude guardar: ${esc(String(e))}</div>`,
      );
    } finally {
      btn.disabled = false;
    }
  };

  // Lazy first load: the outer tab switcher (detail_template.js) already
  // toggles `.detail-tab-panel.active` on click — this only decides
  // whether that first click also kicks off a search, so opening the
  // detail window at all doesn't spend an API call on a tab nobody looked at.
  const outerTab = document.querySelector('.detail-tab[data-dp="arte"]');
  if (outerTab) {
    outerTab.addEventListener("click", () => {
      if (searched) return;
      searched = true;
      if (selection.griddb_game_id) {
        loadTab(activeTab, false);
      } else {
        doSearch(gameName);
      }
    });
  }

  // Fire-and-forget: a local HashMap lookup, not worth blocking the
  // handlers above on — by the time a human actually clicks the Arte tab
  // this has essentially always already resolved.
  invoke("get_artwork", { id })
    .then(current => {
      if (targetId !== id) return; // detail window retargeted mid-flight
      if (current) {
        selection = { ...emptySelection(), ...current };
        renderSelectedRow();
      }
    })
    .catch(() => {});
}

// Same Filters-button-opens-a-panel-with-Apply/Cancel shape as capydeploy's
// own FiltersModal — edits are a draft until Aplicar; opening the panel (or
// Cancelar) always shows/restores the last APPLIED state, never a stale
// half-edited one from a previous open-close.
function installFiltersPanel() {
  const btn = document.getElementById("artworkFiltersBtn");
  const panel = document.getElementById("artworkFiltersPanel");
  const animCb = document.getElementById("artworkShowAnimated");
  const staticCb = document.getElementById("artworkShowStatic");
  const nsfwCb = document.getElementById("artworkShowNsfw");
  const humorCb = document.getElementById("artworkShowHumor");

  btn.onclick = () => {
    animCb.checked = appliedAnimated;
    staticCb.checked = appliedStatic;
    nsfwCb.checked = appliedNsfw;
    humorCb.checked = appliedHumor;
    panel.classList.toggle("hidden");
  };
  document.getElementById("artworkFiltersCancel").onclick = () => {
    panel.classList.add("hidden");
  };
  // Same guard as capydeploy's own FiltersModal — unchecking both would
  // silently hide every result, so the last checked box can't be turned off.
  const guardBothOff = cb => {
    if (!animCb.checked && !staticCb.checked) cb.checked = true;
  };
  animCb.onchange = () => guardBothOff(staticCb);
  staticCb.onchange = () => guardBothOff(animCb);
  document.getElementById("artworkFiltersApply").onclick = () => {
    appliedAnimated = animCb.checked;
    appliedStatic = staticCb.checked;
    appliedNsfw = nsfwCb.checked;
    appliedHumor = humorCb.checked;
    imageType = appliedAnimated && appliedStatic ? "" : appliedAnimated ? "animated" : "static";
    panel.classList.add("hidden");
    imageCache = {};
    loadTab(activeTab, false);
  };
}

async function doSearch(term) {
  const trimmed = term.trim();
  const resultsEl = document.getElementById("artworkSearchResults");
  if (!trimmed) {
    resultsEl.innerHTML = "";
    return;
  }
  resultsEl.innerHTML = `<div class="loading"><div class="spinner"></div></div>`;
  try {
    const results = await invoke("steamgriddb_search", { term: trimmed });
    if (!results.length) {
      resultsEl.innerHTML = `<div class="cartridge-guide">Sin resultados para "${esc(trimmed)}".</div>`;
      return;
    }
    resultsEl.innerHTML = results
      .map(
        r =>
          `<div class="collection-row" data-gid="${r.id}">` +
          `<span class="collection-name">${esc(r.name)}${r.verified ? " ✓" : ""}</span>` +
          `</div>`,
      )
      .join("");
    resultsEl.onclick = e => {
      const row = e.target.closest(".collection-row");
      if (!row) return;
      selection.griddb_game_id = parseInt(row.dataset.gid, 10);
      imageCache = {};
      loadTab(activeTab, false);
    };
  } catch (e) {
    resultsEl.innerHTML = `<div class="cartridge-warn">${esc(String(e))}</div>`;
  }
}

// SteamGridDB pages at 50 per request (confirmed live) — `hasMore` is a
// length heuristic, same one capydeploy's own `artworkTab.svelte.ts` uses
// (`loaded.length >= 50`), not a real total count from the API.
const PAGE_SIZE = 50;

async function loadTab(tabKey, append) {
  const gridEl = document.getElementById("artworkGrid");
  if (!selection.griddb_game_id) {
    gridEl.innerHTML = `<div class="loading">Elegí un resultado de la búsqueda primero.</div>`;
    return;
  }
  const cache = imageCache[tabKey] || { items: [], page: 0, hasMore: false };
  if (!append) {
    if (imageCache[tabKey]) {
      renderGrid();
      return;
    }
    cache.items = [];
    cache.page = 0;
  }
  if (!append) gridEl.innerHTML = `<div class="loading"><div class="spinner"></div></div>`;
  try {
    const raw = await invoke(TABS[tabKey].fetch, {
      gameId: selection.griddb_game_id,
      page: cache.page,
      filters: { imageType, nsfw: appliedNsfw, humor: appliedHumor },
    });
    const filtered = TABS[tabKey].filter ? raw.filter(TABS[tabKey].filter) : raw;
    cache.items = append ? [...cache.items, ...filtered] : filtered;
    cache.hasMore = raw.length >= PAGE_SIZE;
    cache.page += 1;
    imageCache[tabKey] = cache;
    renderGrid();
  } catch (e) {
    gridEl.innerHTML = `<div class="cartridge-warn">${esc(String(e))}</div>`;
  }
}

function isAnimatedThumb(thumb) {
  return !!thumb && thumb.includes(".webm");
}

function renderGrid() {
  const gridEl = document.getElementById("artworkGrid");
  const loadMoreEl = document.getElementById("artworkLoadMoreRow");
  const tab = TABS[activeTab];
  const cache = imageCache[activeTab] || { items: [], hasMore: false };
  if (!cache.items.length) {
    gridEl.innerHTML = `<div class="loading">Sin ${esc(tab.label)} para este juego.</div>`;
    loadMoreEl.innerHTML = "";
    return;
  }
  const selectedUrl = selection[tab.slot];
  gridEl.innerHTML = cache.items
    .map(img => {
      const isSelected = img.url === selectedUrl;
      const anim = isAnimatedThumb(img.thumb);
      // Root cause found (not just a workaround): WebKitGTK's GStreamer
      // pipeline was hitting real VA-API hardware video decode, a known
      // crashy combination on unstable drivers (confirmed against public
      // GStreamer/WebKitGTK bug reports) — same class of bug regardless of
      // how many videos play at once. `main.rs` now forces software decode
      // (`LIBVA_DRIVER_NAME` set to a bogus driver before GTK/WebKit init),
      // so playback here is safe again. Still hover-to-play, not autoplay —
      // software-decoding 50 looping videos at once would just be slow for
      // no benefit, nobody looks at 50 thumbnails simultaneously anyway.
      const media = anim
        ? `<video src="${esc(img.thumb)}" muted loop playsinline preload="none"></video>`
        : `<img src="${esc(img.thumb || img.url)}" loading="lazy">`;
      return (
        `<div class="art-thumb${isSelected ? " selected" : ""}" data-url="${esc(img.url)}">` +
        media +
        (anim ? `<span class="art-anim-badge">ANIM</span>` : "") +
        `</div>`
      );
    })
    .join("");
  gridEl.querySelectorAll("video").forEach(v => {
    v.addEventListener("mouseenter", () => v.play().catch(() => {}));
    v.addEventListener("mouseleave", () => v.pause());
  });
  loadMoreEl.innerHTML = cache.hasMore
    ? `<button class="cartridge-btn-secondary art-load-more" id="artworkLoadMore">Cargar más</button>`
    : "";
  loadMoreEl.onclick = e => {
    if (e.target.id === "artworkLoadMore") loadTab(activeTab, true);
  };
  gridEl.onclick = e => {
    const thumb = e.target.closest(".art-thumb");
    if (!thumb) return;
    const url = thumb.dataset.url;
    // Click again to deselect — same toggle capydeploy's own picker allows.
    selection[tab.slot] = selection[tab.slot] === url ? "" : url;
    renderGrid();
    renderSelectedRow();
  };
}

function renderSelectedRow() {
  const el = document.getElementById("artworkSelectedRow");
  el.innerHTML = Object.values(TABS)
    .map(tab => {
      const url = selection[tab.slot];
      const preview = url ? `<img src="${esc(url)}">` : `<div class="art-empty"></div>`;
      return `<div class="art-selected-slot"><span>${esc(tab.label)}</span>${preview}</div>`;
    })
    .join("");
}
