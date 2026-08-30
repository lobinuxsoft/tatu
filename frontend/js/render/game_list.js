import { esc, gLetter } from "../utils.js";
import { AL } from "../state.js";

// The real list-rendering engine — extracted out of what used to be
// Steam-only code (render/steam.js) so a second data source (GOG) uses the
// SAME alphabetical grouping / per-letter progress bar / row markup
// instead of a stripped-down re-implementation (#243, live feedback: the
// first GOG pass "no tiene nivel de comparación" with Steam's list because
// it never went through this engine at all). Steam keeps its own category
// tabs / preservability filter / HLTB-duration sort — those are real
// Steam-specific data dimensions (Steam app categories, PCGamingWiki DRM
// classification) with no GOG equivalent, not part of the list template.

/// One row — checkbox, thumbnail, name, tag pills, right-hand column.
export function buildGameRow({ id, listKey, chk, name, imgHtml = "", extraNameHtml = "", tagsHtml = "", rightText = "—" }) {
  const tags = tagsHtml ? `<div class="tags">${tagsHtml}</div>` : "";
  return (
    `<tr class="${chk ? "done" : ""}">` +
      `<td><input type="checkbox" data-id="${id}" data-list="${listKey}" ${chk ? "checked" : ""}></td>` +
      `<td><div class="game-cell"><span class="gn">${imgHtml}${esc(name)}${extraNameHtml}</span>${tags}</div></td>` +
      `<td>${rightText}</td>` +
    `</tr>`
  );
}

/// Groups `items` (each needs `.name` and `.chk`) by first letter, renders
/// one `.letter-group` section per letter with its own progress bar into
/// `contentEl`, and (if `navEl` given) the A-Z jump nav highlighting only
/// the letters actually present — the exact structure `renderSteam()` used
/// to build inline.
export function renderLetterGroupedList({ items, buildRowHtml, contentEl, navEl, rightColumnHeader = "Horas", emptyHtml }) {
  contentEl.innerHTML = "";

  const groups = {};
  const stats = {};
  items.forEach(g => {
    const L = gLetter(g.name);
    if (!groups[L]) { groups[L] = []; stats[L] = { t: 0, d: 0 }; }
    groups[L].push(g);
    stats[L].t++;
    if (g.chk) stats[L].d++;
  });

  if (navEl) {
    navEl.innerHTML = AL.map(l => `<a href="#g-${l}" class="${groups[l] ? "hl" : ""}">${l}</a>`).join("");
  }

  Object.keys(groups)
    .sort((a, b) => (a === "#" ? -1 : b === "#" ? 1 : a.localeCompare(b)))
    .forEach(L => {
      const gg = groups[L];
      const st = stats[L];
      const pct = st.t ? Math.round((st.d / st.t) * 100) : 0;
      const sec = document.createElement("div");
      sec.className = "letter-group";
      sec.id = "g-" + L;
      const rows = gg.map(buildRowHtml).join("");
      sec.innerHTML =
        `<div class="letter-header">` +
          `<div class="letter-big">${esc(L)}</div>` +
          `<div class="letter-info">${st.d}/${st.t}</div>` +
          `<div class="letter-bar"><div class="letter-bar-fill" style="width:${pct}%"></div></div>` +
        `</div>` +
        `<table><thead><tr><th></th><th>Juego</th><th>${esc(rightColumnHeader)}</th></tr></thead><tbody>${rows}</tbody></table>`;
      contentEl.appendChild(sec);
    });

  if (Object.keys(groups).length === 0 && emptyHtml) {
    contentEl.innerHTML = emptyHtml;
  }
}
