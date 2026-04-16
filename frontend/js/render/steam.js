import { state, TL, TC, AL } from "../state.js";
import { esc, formatBytes, gCat, gLetter } from "../utils.js";
import { renderDrmInlineBadge } from "../panel/drm_view.js";

export function renderSteam() {
  const content = document.getElementById("content");
  const nav = document.getElementById("lNav");
  document.querySelectorAll(".tbtn").forEach(b => b.classList.toggle("on", !!state.tog[b.dataset.t]));
  const cc = { game: 0, mp: 0, tool: 0, demo: 0 };
  state.G.forEach(g => { cc[gCat(g)]++; });
  document.getElementById("cG").textContent = "(" + cc.game + ")";
  document.getElementById("cM").textContent = "(" + cc.mp + ")";
  document.getElementById("cT").textContent = "(" + cc.tool + ")";
  document.getElementById("cD").textContent = "(" + cc.demo + ")";
  const favCount = state.G.filter(g => state.favorites.has(g.id)).length;
  document.getElementById("cFav").textContent = "(" + favCount + ")";
  document.getElementById("tabSteamCount").textContent = "(" + state.G.length + ")";

  document.querySelectorAll("#sortRow .sbtn").forEach(b => b.classList.toggle("active", b.dataset.sort === state.sortMode));

  let comp = 0, vis = 0, hrs = 0, bytes = 0;
  const filtered = [];
  state.G.forEach(g => {
    const cat = gCat(g), chk = state.completed.has(g.id);
    if (!state.tog[cat]) return;
    if (state.q && !g.name.toLowerCase().includes(state.q) && !(g.genres && g.genres.some(x => x.toLowerCase().includes(state.q)))) return;
    if (state.sf === "done" && !chk) return;
    if (state.sf === "pending" && chk) return;
    if (state.sf === "unplayed" && (g.hours > 0 || chk)) return;
    if (state.sf === "favorites" && !state.favorites.has(g.id)) return;
    if (state.pf !== "all") {
      const info = state.drmCache[g.id];
      const k = info && info.preservability ? info.preservability.kind : "unknown";
      if (k !== state.pf) return;
    }
    vis++; if (chk) comp++; hrs += g.hours;
    if (state.sizeCache[g.id]) bytes += state.sizeCache[g.id].bytes;
    filtered.push({ ...g, chk });
  });

  const hltbField = state.sortMode === "main" ? "main_hours"
    : state.sortMode === "extra" ? "extra_hours"
    : state.sortMode === "comp" ? "completionist_hours"
    : null;
  const sortByDuration = hltbField !== null;

  if (sortByDuration) {
    filtered.sort((a, b) => {
      const ha = state.hltbCache[a.id] ? (state.hltbCache[a.id][hltbField] || 0) : -1;
      const hb = state.hltbCache[b.id] ? (state.hltbCache[b.id][hltbField] || 0) : -1;
      if (ha === -1 && hb === -1) return a.name.localeCompare(b.name);
      if (ha === -1) return 1;
      if (hb === -1) return 1;
      return ha - hb;
    });
  }

  content.innerHTML = "";

  if (sortByDuration) {
    nav.innerHTML = "";
    let rows = "";
    filtered.forEach(g => { rows += buildRow(g); });
    if (rows) {
      const sec = document.createElement("div");
      sec.className = "letter-group";
      sec.innerHTML = `<table><thead><tr><th></th><th>Juego</th><th>Horas</th></tr></thead><tbody>${rows}</tbody></table>`;
      content.appendChild(sec);
    }
  } else {
    const groups = {}, gs = {};
    filtered.forEach(g => {
      const L = gLetter(g.name);
      if (!groups[L]) { groups[L] = []; gs[L] = { t: 0, d: 0 }; }
      groups[L].push(g); gs[L].t++; if (g.chk) gs[L].d++;
    });
    nav.innerHTML = AL.map(l => `<a href="#g-${l}" class="${groups[l] ? "hl" : ""}">${l}</a>`).join("");
    Object.keys(groups).sort((a, b) => a === "#" ? -1 : b === "#" ? 1 : a.localeCompare(b)).forEach(L => {
      const gg = groups[L], st = gs[L], pct = st.t ? Math.round(st.d / st.t * 100) : 0;
      const sec = document.createElement("div");
      sec.className = "letter-group"; sec.id = "g-" + L;
      let rows = "";
      gg.forEach(g => { rows += buildRow(g); });
      sec.innerHTML = `<div class="letter-header"><div class="letter-big">${L}</div><div class="letter-info">${st.d}/${st.t}</div><div class="letter-bar"><div class="letter-bar-fill" style="width:${pct}%"></div></div></div><table><thead><tr><th></th><th>Juego</th><th>Horas</th></tr></thead><tbody>${rows}</tbody></table>`;
      content.appendChild(sec);
    });
  }

  if (vis === 0 && state.G.length > 0) {
    content.innerHTML = '<div class="loading" style="color:#8b949e">No hay juegos con estos filtros.</div>';
  }
  const pct = vis ? Math.round(comp / vis * 100) : 0;
  document.getElementById("sComp").textContent = comp;
  document.getElementById("sPend").textContent = vis - comp;
  document.getElementById("sHours").textContent = Math.round(hrs).toLocaleString();
  document.getElementById("sVis").textContent = vis;
  document.getElementById("sSize").textContent = bytes > 0 ? formatBytes(bytes) : "\u2014";
  document.getElementById("pBar").style.width = pct + "%";
  document.getElementById("pText").textContent = pct + "% (" + comp + "/" + vis + ")";
}

function buildRow(g) {
  const img = g.icon
    ? `<img src="https://media.steampowered.com/steamcommunity/public/images/apps/${g.id}/${g.icon}.jpg" loading="lazy">`
    : "";
  const h = g.hours > 0 ? g.hours + "h" : "\u2014";
  let tagsHtml = "";
  if (g.tag) tagsHtml += `<span class="tag ${TC[g.tag]}">${TL[g.tag]}</span>`;
  if (g.achievements > 0) {
    const ap = state.achProgress[g.id];
    if (ap) {
      const apPct = ap[1] ? Math.round(ap[0] / ap[1] * 100) : 0;
      tagsHtml += `<span class="mini-ach-bar">\u{1F3C6} ${ap[0]}/${ap[1]} <span class="mini-ach-track"><span class="mini-ach-fill" style="width:${apPct}%"></span></span></span>`;
    } else {
      tagsHtml += `<span class="tag tag-ach">\u{1F3C6} ${g.achievements}</span>`;
    }
  }
  if (g.has_cards) tagsHtml += `<span class="tag tag-cards">\u{1F0CF} Cromos</span>`;
  if (g.genres && g.genres.length) {
    g.genres.forEach(x => { tagsHtml += `<span class="tag tag-genre">${esc(x)}</span>`; });
  }
  const hl = state.hltbCache[g.id];
  if (hl) {
    if (hl.main_hours > 0) tagsHtml += `<span class="tag tag-genre">\u{1F552} Historia: ${hl.main_hours}h</span>`;
    if (hl.extra_hours > 0) tagsHtml += `<span class="tag tag-ach">\u{1F552} Main+Extra: ${hl.extra_hours}h</span>`;
    if (hl.completionist_hours > 0) tagsHtml += `<span class="tag tag-cards">\u{1F552} 100%: ${hl.completionist_hours}h</span>`;
  }
  const sz = state.sizeCache[g.id];
  if (sz && sz.bytes > 0) {
    const isAppinfo = sz.source && sz.source.kind === "appinfo";
    const cls = isAppinfo ? "tag-size-estimate" : "tag-size";
    const tip = isAppinfo
      ? "Upper bound estimado desde appinfo.vdf (suma de depots)"
      : "Tamaño exacto en disco (libraryfolders.vdf)";
    const prefix = isAppinfo ? "~" : "";
    tagsHtml += `<span class="tag ${cls}" title="${esc(tip)}">\u{1F4BE} ${prefix}${formatBytes(sz.bytes)}</span>`;
  }
  const tagsRow = tagsHtml ? `<div class="tags">${tagsHtml}</div>` : "";
  const drmInline = renderDrmInlineBadge(state.drmCache[g.id]);
  return `<tr class="${g.chk ? "done" : ""}"><td><input type="checkbox" data-id="${g.id}" data-list="steam" ${g.chk ? "checked" : ""}></td><td><div class="game-cell"><span class="gn">${img}${esc(g.name)}${drmInline}</span>${tagsRow}</div></td><td>${h}</td></tr>`;
}
