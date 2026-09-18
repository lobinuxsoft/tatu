import { state } from "../state.js";
import { esc } from "../utils.js";
import { buildGameRow, matchesQuery, renderLetterGroupedList } from "./game_list.js";
import { renderDrmInlineBadge } from "../panel/drm_view.js";

const EMPTY_HTML = '<div class="empty-state">Conectá tu cuenta de Epic Games en <strong>Settings</strong> y dale a "Actualizar biblioteca".</div>';
const NO_MATCH_HTML = '<div class="loading" style="color:#8b949e">No hay juegos con estos filtros.</div>';

// Same alphabetical-grouped list engine Steam/GOG's tabs run on
// (render/game_list.js) — filtered by name/developer search.
export function renderEgs() {
  document.getElementById("tabEgsCount").textContent = "(" + state.EGS.length + ")";

  const q = state.egsQ;
  const filtered = state.EGS.filter(g => !q || matchesQuery(g, q));

  let comp = 0;
  const items = filtered.map(g => {
    const chk = state.completedEgs.has(g.id);
    if (chk) comp++;
    return { ...g, name: g.title, chk };
  });

  const contentEl = document.getElementById("egsContent");
  const navEl = document.getElementById("lNavEgs");

  if (state.EGS.length === 0) {
    contentEl.innerHTML = EMPTY_HTML;
    navEl.innerHTML = "";
  } else {
    renderLetterGroupedList({
      items,
      buildRowHtml: buildRow,
      contentEl,
      navEl,
      rightColumnHeader: "Developer",
      emptyHtml: NO_MATCH_HTML,
    });
  }

  const total = items.length;
  const pct = total ? Math.round((comp / total) * 100) : 0;
  document.getElementById("egsComp").textContent = comp;
  document.getElementById("egsPend").textContent = total - comp;
  document.getElementById("egspBar").style.width = pct + "%";
  document.getElementById("egspText").textContent = pct + "% (" + comp + "/" + total + ")";
}

// No genre tags — EGS's catalog exposes no genre field (checked live, see
// egs_account::mod.rs). Right column shows developer instead of a year.
function buildRow(g) {
  const img = g.icon_url ? `<img src="${esc(g.icon_url)}" loading="lazy">` : "";
  let tagsHtml = "";
  if (state.cartridgeCache.has(`egs:${g.id}`)) tagsHtml += `<span class="tag tag-cartridge">\u{1F4BF} En cartucho</span>`;
  const dev = (g.developers && g.developers[0]) || "—";
  return buildGameRow({
    id: g.id,
    listKey: "egs",
    chk: g.chk,
    name: g.title,
    imgHtml: img,
    extraNameHtml: renderDrmInlineBadge(state.egsDrmCache[g.id]),
    tagsHtml,
    rightText: esc(dev),
  });
}
