import { state } from "../state.js";
import { esc } from "../utils.js";
import { buildGameRow, renderLetterGroupedList } from "./game_list.js";

const EMPTY_HTML = '<div class="empty-state">Conectá tu cuenta de GOG en <strong>Settings</strong> y dale a "Actualizar biblioteca".</div>';
const NO_MATCH_HTML = '<div class="loading" style="color:#8b949e">No hay juegos con estos filtros.</div>';

// Same alphabetical-grouped list engine Steam's tab runs on
// (render/game_list.js) — filtered by name/genre search, same as Steam's
// own `state.q` filter does for its own list.
export function renderGog() {
  document.getElementById("tabGogCount").textContent = "(" + state.GOG.length + ")";

  const q = state.gogQ;
  const filtered = state.GOG.filter(g => {
    if (!q) return true;
    if (g.title.toLowerCase().includes(q)) return true;
    return g.genres && g.genres.some(x => x.toLowerCase().includes(q));
  });

  let comp = 0;
  const items = filtered.map(g => {
    const chk = state.completedGog.has(g.id);
    if (chk) comp++;
    return { ...g, name: g.title, chk };
  });

  const contentEl = document.getElementById("gogContent");
  const navEl = document.getElementById("lNavGog");

  if (state.GOG.length === 0) {
    contentEl.innerHTML = EMPTY_HTML;
    navEl.innerHTML = "";
  } else {
    renderLetterGroupedList({
      items,
      buildRowHtml: buildRow,
      contentEl,
      navEl,
      rightColumnHeader: "Año",
      emptyHtml: NO_MATCH_HTML,
    });
  }

  const total = items.length;
  const pct = total ? Math.round((comp / total) * 100) : 0;
  document.getElementById("gogComp").textContent = comp;
  document.getElementById("gogPend").textContent = total - comp;
  document.getElementById("gogpBar").style.width = pct + "%";
  document.getElementById("gogpText").textContent = pct + "% (" + comp + "/" + total + ")";
}

// Genre tags computed here (GOG-specific data), handed to the same shared
// row template Steam's own buildRow (render/steam.js) uses.
function buildRow(g) {
  const img = g.icon_url ? `<img src="${esc(g.icon_url)}" loading="lazy">` : "";
  const tagsHtml = (g.genres || []).map(x => `<span class="tag tag-genre">${esc(x)}</span>`).join("");
  const year = g.release_date ? g.release_date.slice(0, 4) : "—";
  return buildGameRow({
    id: g.id,
    listKey: "gog",
    chk: g.chk,
    name: g.title,
    imgHtml: img,
    tagsHtml,
    rightText: year,
  });
}
