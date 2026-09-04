import { esc } from "../utils.js";

// Shared building blocks for the detail window's markup — extracted so
// Steam's own renderer (loaders.js/detail.js) and GOG's (gog_detail.js)
// fill the SAME templates with their own data instead of each hand-writing
// its own copy of the same `.detail-header`/`.detail-info-row`/
// `.detail-tags`/`.cards-grid` HTML strings (#243, live feedback: GOG's
// first pass duplicated this instead of reusing it).

export function headerImg(url) {
  return url ? `<img class="detail-header-img" src="${esc(url)}" alt="">` : "";
}

export function detailHeaderShell(title, metaHtml, imgHtml = "") {
  return (
    `<div class="detail-header">` +
      `<div id="dpHeaderImg">${imgHtml}</div>` +
      `<div class="detail-header-info">` +
        `<div class="detail-title">${esc(title)}</div>` +
        `<div class="detail-meta">${metaHtml}</div>` +
      `</div>` +
    `</div>`
  );
}

// The label+value pair alone, with no wrapping `.detail-info-row` — for
// replacing just the inside of a row `infoRow()` already created (HLTB/DRM
// rows start as a placeholder row, then swap only this inner pair once
// their own async fetch resolves, keeping the same outer row/id).
export function infoRowInner(label, valueHtml, valueTag = "span", extraClass = "") {
  const cls = extraClass ? `detail-info-value ${extraClass}` : "detail-info-value";
  return `<span class="detail-info-label">${esc(label)}</span><${valueTag} class="${cls}">${valueHtml}</${valueTag}>`;
}

export function infoRow(label, valueHtml, id) {
  const idAttr = id ? ` id="${id}"` : "";
  return `<div class="detail-info-row"${idAttr}>${infoRowInner(label, valueHtml)}</div>`;
}

export function tagsRow(label, tags, tagClass = "tag-genre") {
  if (!tags || !tags.length) return "";
  const pills = tags.map(t => `<span class="tag ${tagClass}">${esc(t)}</span>`).join("");
  return `<div class="detail-info-row"><span class="detail-info-label">${esc(label)}</span><div class="detail-tags">${pills}</div></div>`;
}

export function cardGridItem(imgUrl, alt) {
  return `<div class="card-item"><img class="card-img" src="${esc(imgUrl)}" alt="${esc(alt)}" loading="lazy"></div>`;
}

export function cardGrid(label, itemsHtml) {
  if (!itemsHtml.length) return "";
  return (
    `<div class="detail-info-row"><span class="detail-info-label">${esc(label)}</span></div>` +
    `<div class="cards-grid">${itemsHtml.join("")}</div>`
  );
}

export function loadingPlaceholder(text) {
  return `<div class="loading"><div class="spinner"></div><br>${esc(text)}</div>`;
}

// The whole tab bar + one panel per tab, GENUINELY the same markup/switch
// logic for every detail window (#243, live feedback: the window itself —
// tabs included — needs to be one template, not just the header). Steam
// passes 2-4 tabs (Info always, Logros/Cromos/Cheats when the game has
// that data); GOG passes just Info — the switcher below works the same
// either way, including the degenerate one-tab case.
export function detailTabsShell(tabs) {
  const tabsHtml = tabs
    .map((t, i) => `<div class="detail-tab${i === 0 ? " active" : ""}" data-dp="${t.key}">${esc(t.label)}</div>`)
    .join("");
  const panelsHtml = tabs
    .map((t, i) => `<div class="detail-tab-panel${i === 0 ? " active" : ""}" id="${panelId(t.key)}">${t.initialHtml || ""}</div>`)
    .join("");
  return `<div class="detail-tabs">${tabsHtml}</div>` + panelsHtml;
}

// Wires tab clicks for whatever `.detail-tab`/`.detail-tab-panel` set
// `detailTabsShell()` just wrote into `#detailContent` — call once per
// render, after the shell's HTML is in the DOM.
export function installDetailTabSwitcher() {
  const tabsEl = document.querySelector(".detail-tabs");
  if (!tabsEl) return;
  tabsEl.onclick = e => {
    const tab = e.target.closest(".detail-tab");
    if (!tab) return;
    document.querySelectorAll(".detail-tab").forEach(t => t.classList.remove("active"));
    document.querySelectorAll(".detail-tab-panel").forEach(p => p.classList.remove("active"));
    tab.classList.add("active");
    document.getElementById(panelId(tab.dataset.dp)).classList.add("active");
  };
}

function panelId(key) {
  return "dp" + key.charAt(0).toUpperCase() + key.slice(1);
}
