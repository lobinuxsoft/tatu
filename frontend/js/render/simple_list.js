import { esc } from "../utils.js";

// Shared by every "flat list of games with just a name + completed
// checkbox" tab (Non-Steam, GOG, and any future one of the same shape) —
// extracted from what used to be Non-Steam's own renderer (#243) so a
// second tab of the same kind doesn't hand-roll its own copy of the same
// table/stats/empty-state logic.
export function renderSimpleGameList({
  items,
  completedSet,
  listKey,
  countEl,
  contentEl,
  compEl,
  pendEl,
  barEl,
  textEl,
  emptyHtml,
  subtitleOf,
  imgUrlOf,
  tagsOf,
  rightColumnOf,
  rightColumnHeader = "Horas",
}) {
  if (countEl) countEl.textContent = "(" + items.length + ")";

  if (items.length === 0) {
    contentEl.innerHTML = emptyHtml;
    compEl.textContent = "0";
    pendEl.textContent = "0";
    barEl.style.width = "0%";
    textEl.textContent = "0%";
    return;
  }

  let comp = 0;
  let rows = "";
  items.forEach(g => {
    const chk = completedSet.has(g.id);
    if (chk) comp++;
    const subtitle = subtitleOf ? subtitleOf(g) : "";
    const sub = subtitle ? `<div class="exe-path" title="${esc(subtitle)}">${esc(subtitle)}</div>` : "";
    const imgUrl = imgUrlOf ? imgUrlOf(g) : "";
    const img = imgUrl ? `<img src="${esc(imgUrl)}" loading="lazy">` : "";
    const right = rightColumnOf ? rightColumnOf(g) : "—";
    const tagList = tagsOf ? tagsOf(g) : [];
    const tags = tagList.length
      ? `<div class="tags">${tagList.map(t => `<span class="tag tag-genre">${esc(t)}</span>`).join("")}</div>`
      : "";
    rows += `<tr class="${chk ? "done" : ""}"><td><input type="checkbox" data-id="${g.id}" data-list="${listKey}" ${chk ? "checked" : ""}></td><td><div class="game-cell"><span class="gn">${img}${esc(g.name)}</span>${sub}${tags}</div></td><td>${right}</td></tr>`;
  });

  const total = items.length;
  const pct = Math.round(comp / total * 100);
  contentEl.innerHTML = `<table><thead><tr><th></th><th>Juego</th><th>${esc(rightColumnHeader)}</th></tr></thead><tbody>${rows}</tbody></table>`;
  compEl.textContent = comp;
  pendEl.textContent = total - comp;
  barEl.style.width = pct + "%";
  textEl.textContent = pct + "% (" + comp + "/" + total + ")";
}
