const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getVersion } = window.__TAURI__.app;

// --- State ---
let G = [], NS = [], completed = new Set(), completedNS = new Set(), achProgress = {};
let tog = { game: true, mp: false, tool: false, demo: false }, sf = "all", q = "";
let favorites = new Set(), hltbCache = {}, sortMode = "alpha";
let panelOpen = false, panelGameId = null;
let loadingTasks = new Set();
function updateGlobalLoading() {
  const el = document.getElementById("globalLoading");
  if (!el) return;
  if (loadingTasks.size === 0) {
    el.classList.add("hidden");
  } else {
    el.classList.remove("hidden");
    const msgs = [];
    if (loadingTasks.has("info")) msgs.push("info del juego");
    if (loadingTasks.has("logros")) msgs.push("logros");
    if (loadingTasks.has("cromos")) msgs.push("cromos e insignias");
    el.querySelector(".global-loading-text").textContent = `Cargando ${msgs.join(", ")}...`;
  }
}
function startLoading(task) { loadingTasks.add(task); updateGlobalLoading(); }
function doneLoading(task) { loadingTasks.delete(task); updateGlobalLoading(); }
const TL = { tool: "Tool", mp: "MP", demo: "Demo" };
const TC = { tool: "tag-tool", mp: "tag-mp", demo: "tag-demo" };
const gCat = g => g.tag || "game";
const gLetter = n => { const f = n.charAt(0).toUpperCase(); return /[A-Z]/.test(f) ? f : "#"; };
function esc(s) { return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;'); }
function fmtDate(epoch) {
  if (!epoch) return "";
  return new Date(epoch * 1000).toLocaleDateString("es-AR", { day: "numeric", month: "short", year: "numeric" });
}
function mcColor(score) { return score >= 75 ? "mc-green" : score >= 50 ? "mc-yellow" : "mc-red"; }

// --- Tabs ---
document.querySelector(".tabs").addEventListener("click", e => {
  const tab = e.target.closest(".tab"); if (!tab) return;
  document.querySelectorAll(".tab").forEach(t => t.classList.remove("active"));
  document.querySelectorAll(".tab-panel").forEach(p => p.classList.remove("active"));
  tab.classList.add("active");
  document.getElementById("panel-" + tab.dataset.tab).classList.add("active");
});

// --- Steam render ---
function renderSteam() {
  const content = document.getElementById("content"), nav = document.getElementById("lNav");
  document.querySelectorAll(".tbtn").forEach(b => b.classList.toggle("on", !!tog[b.dataset.t]));
  const cc = { game: 0, mp: 0, tool: 0, demo: 0 };
  G.forEach(g => { cc[gCat(g)]++; });
  document.getElementById("cG").textContent = "(" + cc.game + ")";
  document.getElementById("cM").textContent = "(" + cc.mp + ")";
  document.getElementById("cT").textContent = "(" + cc.tool + ")";
  document.getElementById("cD").textContent = "(" + cc.demo + ")";
  const favCount = G.filter(g => favorites.has(g.id)).length;
  document.getElementById("cFav").textContent = "(" + favCount + ")";
  document.getElementById("tabSteamCount").textContent = "(" + G.length + ")";

  document.querySelectorAll("#sortRow .sbtn").forEach(b => b.classList.toggle("active", b.dataset.sort === sortMode));

  let comp = 0, vis = 0, hrs = 0;
  const filtered = [];
  G.forEach(g => {
    const cat = gCat(g), chk = completed.has(g.id);
    if (!tog[cat]) return;
    if (q && !g.name.toLowerCase().includes(q) && !(g.genres && g.genres.some(x => x.toLowerCase().includes(q)))) return;
    if (sf === "done" && !chk) return;
    if (sf === "pending" && chk) return;
    if (sf === "unplayed" && (g.hours > 0 || chk)) return;
    if (sf === "favorites" && !favorites.has(g.id)) return;
    vis++; if (chk) comp++; hrs += g.hours;
    filtered.push({ ...g, chk });
  });

  const hltbField = sortMode === "main" ? "main_hours" : sortMode === "extra" ? "extra_hours" : sortMode === "comp" ? "completionist_hours" : null;
  const sortByDuration = hltbField !== null;

  if (sortByDuration) {
    filtered.sort((a, b) => {
      const ha = hltbCache[a.id] ? (hltbCache[a.id][hltbField] || 0) : -1;
      const hb = hltbCache[b.id] ? (hltbCache[b.id][hltbField] || 0) : -1;
      if (ha === -1 && hb === -1) return a.name.localeCompare(b.name);
      if (ha === -1) return 1;
      if (hb === -1) return 1;
      return ha - hb;
    });
  }

  content.innerHTML = "";

  function buildRow(g) {
    const img = g.icon ? `<img src="https://media.steampowered.com/steamcommunity/public/images/apps/${g.id}/${g.icon}.jpg" loading="lazy">` : "";
    const h = g.hours > 0 ? g.hours + "h" : "\u2014";
    let tagsHtml = "";
    if (g.tag) tagsHtml += `<span class="tag ${TC[g.tag]}">${TL[g.tag]}</span>`;
    if (g.achievements > 0) {
      const ap = achProgress[g.id];
      if (ap) {
        const apPct = ap[1] ? Math.round(ap[0] / ap[1] * 100) : 0;
        tagsHtml += `<span class="mini-ach-bar">\u{1F3C6} ${ap[0]}/${ap[1]} <span class="mini-ach-track"><span class="mini-ach-fill" style="width:${apPct}%"></span></span></span>`;
      } else {
        tagsHtml += `<span class="tag tag-ach">\u{1F3C6} ${g.achievements}</span>`;
      }
    }
    if (g.has_cards) tagsHtml += `<span class="tag tag-cards">\u{1F0CF} Cromos</span>`;
    if (g.genres && g.genres.length) g.genres.forEach(x => { tagsHtml += `<span class="tag tag-genre">${esc(x)}</span>`; });
    const hl = hltbCache[g.id];
    if (hl) {
      if (hl.main_hours > 0) tagsHtml += `<span class="tag tag-genre">\u{1F552} Historia: ${hl.main_hours}h</span>`;
      if (hl.extra_hours > 0) tagsHtml += `<span class="tag tag-ach">\u{1F552} Main+Extra: ${hl.extra_hours}h</span>`;
      if (hl.completionist_hours > 0) tagsHtml += `<span class="tag tag-cards">\u{1F552} 100%: ${hl.completionist_hours}h</span>`;
    }
    const tagsRow = tagsHtml ? `<div class="tags">${tagsHtml}</div>` : "";
    return `<tr class="${g.chk?'done':''}"><td><input type="checkbox" data-id="${g.id}" data-list="steam" ${g.chk?'checked':''}></td><td><div class="game-cell"><span class="gn">${img}${esc(g.name)}</span>${tagsRow}</div></td><td>${h}</td></tr>`;
  }

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
    const AL = ["#","A","B","C","D","E","F","G","H","I","J","K","L","M","N","O","P","Q","R","S","T","U","V","W","X","Y","Z"];
    nav.innerHTML = AL.map(l => `<a href="#g-${l}" class="${groups[l]?'hl':''}">${l}</a>`).join("");
    Object.keys(groups).sort((a,b) => a==="#"?-1:b==="#"?1:a.localeCompare(b)).forEach(L => {
      const gg = groups[L], st = gs[L], pct = st.t ? Math.round(st.d/st.t*100) : 0;
      const sec = document.createElement("div");
      sec.className = "letter-group"; sec.id = "g-" + L;
      let rows = "";
      gg.forEach(g => { rows += buildRow(g); });
      sec.innerHTML = `<div class="letter-header"><div class="letter-big">${L}</div><div class="letter-info">${st.d}/${st.t}</div><div class="letter-bar"><div class="letter-bar-fill" style="width:${pct}%"></div></div></div><table><thead><tr><th></th><th>Juego</th><th>Horas</th></tr></thead><tbody>${rows}</tbody></table>`;
      content.appendChild(sec);
    });
  }

  if (vis === 0 && G.length > 0) content.innerHTML = '<div class="loading" style="color:#8b949e">No hay juegos con estos filtros.</div>';
  const pct = vis ? Math.round(comp/vis*100) : 0;
  document.getElementById("sComp").textContent = comp;
  document.getElementById("sPend").textContent = vis - comp;
  document.getElementById("sHours").textContent = Math.round(hrs).toLocaleString();
  document.getElementById("sVis").textContent = vis;
  document.getElementById("pBar").style.width = pct + "%";
  document.getElementById("pText").textContent = pct + "% (" + comp + "/" + vis + ")";
}

// --- Non-Steam render ---
function renderNonSteam() {
  const content = document.getElementById("nsContent");
  document.getElementById("tabNonsteamCount").textContent = "(" + NS.length + ")";

  if (NS.length === 0) {
    content.innerHTML = '<div class="empty-state">No hay juegos Non-Steam. Dale a "Leer shortcuts.vdf".</div>';
    document.getElementById("nsComp").textContent = "0";
    document.getElementById("nsPend").textContent = "0";
    document.getElementById("nspBar").style.width = "0%";
    document.getElementById("nspText").textContent = "0%";
    return;
  }

  let comp = 0;
  let rows = "";
  NS.forEach(g => {
    const chk = completedNS.has(g.id);
    if (chk) comp++;
    const exePath = g.exe ? `<div class="exe-path" title="${esc(g.exe)}">${esc(g.exe)}</div>` : "";
    rows += `<tr class="${chk?'done':''}"><td><input type="checkbox" data-id="${g.id}" data-list="nonsteam" ${chk?'checked':''}></td><td><div class="game-cell"><span class="gn">${esc(g.name)}</span>${exePath}</div></td><td>\u2014</td></tr>`;
  });

  const total = NS.length;
  const pct = Math.round(comp / total * 100);
  content.innerHTML = `<table><thead><tr><th></th><th>Juego</th><th>Horas</th></tr></thead><tbody>${rows}</tbody></table>`;
  document.getElementById("nsComp").textContent = comp;
  document.getElementById("nsPend").textContent = total - comp;
  document.getElementById("nspBar").style.width = pct + "%";
  document.getElementById("nspText").textContent = pct + "% (" + comp + "/" + total + ")";
}

// --- Actions ---
async function doSync() {
  const btn = document.getElementById("syncBtn");
  btn.disabled = true; btn.textContent = "Sincronizando...";
  try {
    G = await invoke("sync_steam");
    document.getElementById("subtitle").textContent = G.length + " juegos Steam + " + NS.length + " Non-Steam";
    document.getElementById("syncInfo").textContent = `Sincronizado \u2014 ${G.length} juegos`;
    renderSteam();
  } catch (e) { document.getElementById("syncInfo").textContent = "Error: " + e; }
  finally { btn.disabled = false; btn.textContent = "Sincronizar"; }
}

async function doSyncNonSteam() {
  const btn = document.getElementById("nsSyncBtn");
  btn.disabled = true; btn.textContent = "Leyendo...";
  try {
    NS = await invoke("sync_nonsteam");
    document.getElementById("nsSyncInfo").textContent = `${NS.length} juegos Non-Steam encontrados`;
    renderNonSteam();
  } catch (e) { document.getElementById("nsSyncInfo").textContent = "Error: " + e; }
  finally { btn.disabled = false; btn.textContent = "Leer shortcuts.vdf"; }
}

// --- Detail Panel ---
function openDetailPanel(gameId) {
  const g = G.find(x => x.id === gameId);
  if (!g) return;
  panelOpen = true; panelGameId = gameId;

  const h = g.hours > 0 ? g.hours + "h" : "\u2014";

  document.getElementById("detailContent").innerHTML =
    `<div class="detail-header">` +
      `<div id="dpHeaderImg"></div>` +
      `<div class="detail-header-info">` +
        `<div class="detail-title">${esc(g.name)}</div>` +
        `<div class="detail-meta"><span>${h} jugadas</span><span id="dpMetaExtra"></span></div>` +
      `</div>` +
    `</div>` +
    `<div class="detail-tabs">` +
      `<div class="detail-tab active" data-dp="info">Info</div>` +
      `<div class="detail-tab" data-dp="logros">Logros</div>` +
      (g.has_cards ? `<div class="detail-tab" data-dp="cromos">Cromos</div>` : ``) +
    `</div>` +
    `<div class="detail-tab-panel active" id="dpInfo"><div class="loading"><div class="spinner"></div><br>Cargando info...</div></div>` +
    `<div class="detail-tab-panel" id="dpLogros"><div class="loading"><div class="spinner"></div><br>Cargando logros...</div></div>` +
    (g.has_cards ? `<div class="detail-tab-panel" id="dpCromos"><div class="loading"><div class="spinner"></div><br>Cargando inventario de Steam (puede tardar unos segundos)...</div></div>` : ``);

  document.getElementById("detailOverlay").classList.add("open");
  document.getElementById("detailPanel").classList.add("open");

  document.querySelector(".detail-tabs").onclick = e => {
    const tab = e.target.closest(".detail-tab");
    if (!tab) return;
    document.querySelectorAll(".detail-tab").forEach(t => t.classList.remove("active"));
    document.querySelectorAll(".detail-tab-panel").forEach(p => p.classList.remove("active"));
    tab.classList.add("active");
    document.getElementById("dp" + tab.dataset.dp.charAt(0).toUpperCase() + tab.dataset.dp.slice(1)).classList.add("active");
  };

  loadingTasks.clear();
  startLoading("info");
  startLoading("logros");
  loadGameDetails(gameId);
  loadAchievements(gameId);
  if (g.has_cards) { startLoading("cromos"); loadCards(gameId); }
}

async function loadGameDetails(gameId) {
  try {
    const g = await invoke("get_game_details", { appId: gameId });
    if (panelGameId !== gameId) return;

    const idx = G.findIndex(x => x.id === gameId);
    if (idx >= 0) G[idx] = { ...G[idx], ...g };

    const imgEl = document.getElementById("dpHeaderImg");
    if (imgEl && g.header_img) imgEl.innerHTML = `<img class="detail-header-img" src="${g.header_img}" alt="">`;

    const metaEl = document.getElementById("dpMetaExtra");
    if (metaEl) {
      let extra = "";
      if (g.developers && g.developers.length) extra += esc(g.developers.join(", "));
      if (g.metacritic) extra += ` <span class="metacritic-badge ${mcColor(g.metacritic)}">${g.metacritic}</span>`;
      metaEl.innerHTML = extra;
    }

    const infoEl = document.getElementById("dpInfo");
    if (!infoEl) return;

    let html = "";
    if (g.short_description) html += `<div class="detail-desc">${g.short_description}</div>`;

    if (g.genres && g.genres.length) {
      html += `<div class="detail-info-row"><span class="detail-info-label">Generos</span><div class="detail-tags">`;
      g.genres.forEach(x => { html += `<span class="tag tag-genre">${esc(x)}</span>`; });
      html += `</div></div>`;
    }
    if (g.developers && g.developers.length) {
      html += `<div class="detail-info-row"><span class="detail-info-label">Developer</span><span class="detail-info-value">${esc(g.developers.join(", "))}</span></div>`;
    }
    if (g.has_cards) {
      html += `<div class="detail-info-row"><span class="detail-info-label">Cromos</span><span class="detail-info-value"><span class="tag tag-cards">\u{1F0CF} Tiene cromos de Steam</span></span></div>`;
    }
    if (g.metacritic) {
      html += `<div class="detail-info-row"><span class="detail-info-label">Metacritic</span><span class="detail-info-value"><span class="metacritic-badge ${mcColor(g.metacritic)}">${g.metacritic}</span></span></div>`;
    }
    if (g.tag) {
      html += `<div class="detail-info-row"><span class="detail-info-label">Categoria</span><span class="detail-info-value"><span class="tag ${TC[g.tag]}">${TL[g.tag]}</span></span></div>`;
    }
    html += `<div class="detail-info-row"><span class="detail-info-label">Horas</span><span class="detail-info-value">${g.hours > 0 ? g.hours + "h" : "Sin jugar"}</span></div>`;
    html += `<div id="dpHltb" class="detail-info-row"><span class="detail-info-label">Duracion (HLTB)</span><span class="detail-info-value" style="color:#6e7681">Buscando...</span></div>`;
    html += `<div id="dpDrm" class="detail-info-row"><span class="detail-info-label">DRM</span><span class="detail-info-value" style="color:#6e7681">Consultando...</span></div>`;

    if (!html) html = `<div class="ach-empty">No hay detalles disponibles.</div>`;
    infoEl.innerHTML = html;
    loadHltb(g.name, gameId);
    loadDrm(gameId);

    // Dynamically add Cromos tab if details reveal has_cards and tab doesn't exist yet.
    if (g.has_cards && !document.getElementById("dpCromos")) {
      const tabsEl = document.querySelector(".detail-tabs");
      if (tabsEl) {
        const tab = document.createElement("div");
        tab.className = "detail-tab";
        tab.dataset.dp = "cromos";
        tab.textContent = "Cromos";
        tabsEl.appendChild(tab);
      }
      const panel = document.createElement("div");
      panel.className = "detail-tab-panel";
      panel.id = "dpCromos";
      panel.innerHTML = `<div class="loading"><div class="spinner"></div><br>Cargando cromos...</div>`;
      document.getElementById("detailContent").appendChild(panel);
      startLoading("cromos");
      loadCards(gameId);
    }
    doneLoading("info");
  } catch (e) {
    doneLoading("info");
    if (panelGameId !== gameId) return;
    const infoEl = document.getElementById("dpInfo");
    if (infoEl) infoEl.innerHTML = `<div class="ach-empty">Error al cargar info: ${esc(String(e))}</div>`;
  }
}

async function loadAchievements(gameId) {
  const panel = document.getElementById("dpLogros");
  try {
    const data = await invoke("get_game_achievements", { appId: gameId });
    if (panelGameId !== gameId) return;

    const achs = data.achievements || [];
    const unlocked = achs.filter(a => a.achieved);
    const locked = achs.filter(a => !a.achieved);
    const total = achs.length;
    const done = unlocked.length;
    const pct = total ? Math.round(done / total * 100) : 0;

    unlocked.sort((a, b) => b.unlock_time - a.unlock_time);
    locked.sort((a, b) => a.name.localeCompare(b.name));
    const sorted = [...unlocked, ...locked];

    let html = `<div class="ach-progress-wrap"><div class="ach-progress-bar" style="width:${pct}%"></div><div class="ach-progress-text">${done}/${total} \u2014 ${pct}%</div></div>`;
    html += `<ul class="ach-list">`;
    for (const a of sorted) {
      const icon = a.achieved ? a.icon : a.icon_gray;
      const cls = a.achieved ? "" : " locked";
      const date = a.achieved && a.unlock_time ? `<div class="ach-date">${fmtDate(a.unlock_time)}</div>` : "";
      html += `<li class="ach-item${cls}"><img class="ach-icon" src="${icon}" loading="lazy"><div class="ach-info"><div class="ach-name">${esc(a.name)}</div><div class="ach-desc">${esc(a.description)}</div>${date}</div></li>`;
    }
    html += `</ul>`;
    if (panel) panel.innerHTML = html;
    achProgress[gameId] = [done, total];
    doneLoading("logros");
    renderSteam();
  } catch (e) {
    doneLoading("logros");
    if (panelGameId !== gameId) return;
    const errStr = String(e);
    if (errStr.includes("no stats") || errStr.includes("Requested app has no")) {
      if (panel) panel.innerHTML = `<div class="ach-empty">Sin logros</div>`;
    } else if (errStr.includes("not public") || errStr.includes("403")) {
      if (panel) panel.innerHTML = `<div class="ach-empty">Perfil de Steam privado. Cambia la visibilidad a publico para ver logros.</div>`;
    } else {
      if (panel) panel.innerHTML = `<div class="ach-empty">Error al cargar logros: ${esc(errStr)}</div>`;
    }
  }
}

async function loadCards(gameId) {
  const panel = document.getElementById("dpCromos");
  if (!panel) return;
  try {
    const data = await invoke("get_game_cards", { appId: gameId });
    if (panelGameId !== gameId) return;

    const cards = data.cards || [];
    if (cards.length === 0) {
      panel.innerHTML = `<div class="ach-empty">No hay cromos disponibles para este juego.</div>`;
      return;
    }

    const owned = cards.filter(c => c.owned);
    const total = cards.length;
    const pct = total ? Math.round(owned.length / total * 100) : 0;

    let html = "";

    // Badges section.
    const badges = data.badges || [];
    if (badges.length > 0) {
      const normalBadges = badges.filter(b => !b.foil);
      const foilBadges = badges.filter(b => b.foil);
      const earnedCount = normalBadges.filter(b => b.owned).length;

      html += `<div class="badges-section"><div class="cards-section-title">Insignias (${earnedCount}/${normalBadges.length})</div><div class="badges-grid">`;
      for (const b of normalBadges) {
        const cls = b.owned ? "" : " not-earned";
        const img = b.image_url ? `<img class="badge-img" src="${b.image_url}" loading="lazy">` : `<div class="badge-placeholder">${b.level}</div>`;
        html += `<div class="badge-item${cls}">${img}<div class="badge-name">${esc(b.name)}</div><div class="badge-level">Nivel ${b.level}</div><div class="badge-xp">${b.xp} XP</div></div>`;
      }
      html += `</div>`;
      if (foilBadges.length > 0) {
        html += `<div class="cards-section-title foil" style="margin-top:0.8rem">Foil</div><div class="badges-grid">`;
        for (const b of foilBadges) {
          const cls = "foil" + (b.owned ? "" : " not-earned");
          const img = b.image_url ? `<img class="badge-img" src="${b.image_url}" loading="lazy">` : `<div class="badge-placeholder">F</div>`;
          html += `<div class="badge-item ${cls}">${img}<div class="badge-name">${esc(b.name)}</div><div class="badge-xp">${b.xp} XP</div></div>`;
        }
        html += `</div>`;
      }
      html += `</div>`;
    }

    html += `<div class="cards-summary">${owned.length}/${total} cromos \u2014 ${pct}%</div>`;
    html += `<div class="ach-progress-wrap"><div class="ach-progress-bar" style="width:${pct}%"></div><div class="ach-progress-text">${owned.length}/${total}</div></div>`;
    html += `<div class="cards-grid">`;
    for (const c of cards) {
      const cls = c.owned ? "" : " unowned";
      const qty = c.quantity > 1 ? `<span class="card-qty">x${c.quantity}</span>` : "";
      html += `<div class="card-item${cls}"><img class="card-img" src="${c.image_url}" loading="lazy"><div class="card-name">${esc(c.name)}${qty}</div></div>`;
    }
    html += `</div>`;

    panel.innerHTML = html;
    doneLoading("cromos");
  } catch (e) {
    doneLoading("cromos");
    if (panelGameId !== gameId) return;
    const errStr = String(e);
    if (errStr.includes("No trading cards")) {
      if (panel) panel.innerHTML = `<div class="ach-empty">Este juego no tiene cromos.</div>`;
    } else {
      if (panel) panel.innerHTML = `<div class="ach-empty">Error al cargar cromos: ${esc(errStr)}</div>`;
    }
  }
}

function renderDrmBadge(info) {
  if (!info || !info.status) return `<span class="tag tag-drm-unknown">Desconocido</span>`;
  const k = info.status.kind;
  if (k === "drm_free") return `<span class="tag tag-drm-free">DRM-Free</span>`;
  if (k === "steam_only") return `<span class="tag tag-drm-steam">Solo Steam</span>`;
  if (k === "third_party") {
    const vendors = (info.status.vendors || []).map(v => esc(v)).join(", ");
    return `<span class="tag tag-drm-third">DRM 3ros: ${vendors || "desconocido"}</span>`;
  }
  return `<span class="tag tag-drm-unknown">Desconocido</span>`;
}

async function loadDrm(gameId) {
  try {
    const info = await invoke("get_game_drm", { appId: gameId });
    if (panelGameId !== gameId) return;
    const el = document.getElementById("dpDrm");
    if (!el) return;
    const badge = renderDrmBadge(info);
    const title = info && info.notes ? ` title="${esc(info.notes)}"` : "";
    el.innerHTML = `<span class="detail-info-label">DRM</span><span class="detail-info-value"${title}>${badge}</span>`;
  } catch (e) {
    const el = document.getElementById("dpDrm");
    if (el) {
      el.querySelector(".detail-info-value").textContent = "Error";
      el.querySelector(".detail-info-value").style.color = "#f85149";
    }
  }
}

async function loadHltb(gameName, gameId) {
  try {
    const r = await invoke("search_hltb", { appId: gameId, gameName });
    if (panelGameId !== gameId) return;
    const el = document.getElementById("dpHltb");
    if (!el) return;
    if (!r) {
      el.querySelector(".detail-info-value").textContent = "No encontrado";
      el.querySelector(".detail-info-value").style.color = "#484f58";
      return;
    }
    hltbCache[gameId] = r;
    let html = '<div style="display:flex;gap:1rem;flex-wrap:wrap">';
    if (r.main_hours > 0) html += `<span class="tag tag-genre">Historia: ${r.main_hours}h</span>`;
    if (r.extra_hours > 0) html += `<span class="tag tag-ach">Main+Extra: ${r.extra_hours}h</span>`;
    if (r.completionist_hours > 0) html += `<span class="tag tag-cards">100%: ${r.completionist_hours}h</span>`;
    html += '</div>';
    el.innerHTML = `<span class="detail-info-label">Duracion (HLTB)</span><span class="detail-info-value">${html}</span>`;
    renderSteam();
  } catch (_) {
    const el = document.getElementById("dpHltb");
    if (el) { el.querySelector(".detail-info-value").textContent = "Error"; el.querySelector(".detail-info-value").style.color = "#f85149"; }
  }
}

function closeDetailPanel() {
  document.getElementById("detailOverlay").classList.remove("open");
  document.getElementById("detailPanel").classList.remove("open");
  loadingTasks.clear();
  updateGlobalLoading();
  panelOpen = false; panelGameId = null;
}

document.getElementById("detailClose").addEventListener("click", closeDetailPanel);
document.getElementById("detailOverlay").addEventListener("click", closeDetailPanel);
document.addEventListener("keydown", e => { if (e.key === "Escape" && panelOpen) closeDetailPanel(); });

// --- Settings ---
let hasConfig = false;

function loadSettingsUI(apiKey, steamId) {
  document.getElementById("cfgApiKey").value = apiKey || "";
  document.getElementById("cfgSteamId").value = steamId || "";
  hasConfig = !!(apiKey && steamId);
}

function checkConfigWarning() {
  const existing = document.getElementById("configWarning");
  if (existing) existing.remove();
  if (!hasConfig) {
    const warn = document.createElement("div");
    warn.id = "configWarning";
    warn.className = "config-warning";
    warn.innerHTML = 'Configurá tu Steam API Key y Steam ID en la pestaña <strong>Settings</strong> para poder sincronizar.';
    document.getElementById("panel-steam").prepend(warn);
  }
}

document.getElementById("toggleKeyBtn").addEventListener("click", () => {
  const inp = document.getElementById("cfgApiKey");
  const btn = document.getElementById("toggleKeyBtn");
  if (inp.type === "password") { inp.type = "text"; btn.textContent = "Ocultar"; }
  else { inp.type = "password"; btn.textContent = "Mostrar"; }
});

document.getElementById("detectSteamIdBtn").addEventListener("click", async () => {
  const btn = document.getElementById("detectSteamIdBtn");
  btn.disabled = true; btn.textContent = "Detectando...";
  try {
    const id = await invoke("detect_steam_id");
    if (id) {
      document.getElementById("cfgSteamId").value = id;
      btn.textContent = "Detectado";
    } else {
      btn.textContent = "No encontrado";
    }
  } catch (e) { btn.textContent = "Error"; }
  setTimeout(() => { btn.disabled = false; btn.textContent = "Detectar"; }, 3000);
});

document.getElementById("saveSettingsBtn").addEventListener("click", async () => {
  const key = document.getElementById("cfgApiKey").value.trim();
  const id = document.getElementById("cfgSteamId").value.trim();
  const msg = document.getElementById("settingsMsg");
  try {
    await invoke("save_settings", { steamApiKey: key, steamId: id });
    hasConfig = !!(key && id);
    checkConfigWarning();
    msg.style.color = "#2ea043";
    msg.textContent = "Guardado";
    setTimeout(() => { msg.textContent = ""; }, 3000);
  } catch (e) {
    msg.style.color = "#f85149";
    msg.textContent = "Error: " + e;
  }
});

// --- Init ---
async function init() {
  try {
    const data = await invoke("get_state");
    G = data.games || []; completed = new Set(data.completed || []); achProgress = data.ach_progress || {}; hltbCache = data.hltb_cache || {};
    NS = data.non_steam || []; completedNS = new Set(data.completed_nonsteam || []);
    loadSettingsUI(data.steam_api_key, data.steam_id);
    if (!data.steam_id) {
      try {
        const detectedId = await invoke("detect_steam_id");
        if (detectedId) {
          document.getElementById("cfgSteamId").value = detectedId;
        }
      } catch (_) {}
    }
    checkConfigWarning();
    try {
      const favIds = await invoke("get_steam_favorites");
      favorites = new Set(favIds || []);
    } catch (_) { favorites = new Set(); }
    document.getElementById("subtitle").textContent = G.length + " juegos Steam + " + NS.length + " Non-Steam";
    if (G.length > 0) renderSteam();
    else if (hasConfig) await doSync();
    renderNonSteam();
  } catch (e) { document.getElementById("content").innerHTML = '<div class="loading" style="color:#f85149">Error: ' + e + '</div>'; }
}

// --- Events ---
document.getElementById("syncBtn").addEventListener("click", () => {
  if (!hasConfig) { document.getElementById("syncInfo").textContent = "Configurá API Key y Steam ID en Settings primero."; return; }
  doSync();
});
document.getElementById("nsSyncBtn").addEventListener("click", doSyncNonSteam);
document.getElementById("catRow").addEventListener("click", e => { const b = e.target.closest(".tbtn"); if (!b) return; tog[b.dataset.t] = !tog[b.dataset.t]; renderSteam(); });
document.getElementById("statusRow").addEventListener("click", e => { const b = e.target.closest(".sbtn"); if (!b) return; document.querySelectorAll("#statusRow .sbtn").forEach(x => x.classList.remove("active")); b.classList.add("active"); sf = b.dataset.s; renderSteam(); });
document.getElementById("sortRow").addEventListener("click", e => { const b = e.target.closest(".sbtn"); if (!b) return; sortMode = b.dataset.sort; renderSteam(); });
document.addEventListener("change", async e => {
  if (e.target.type === "checkbox" && e.target.dataset.id) {
    const id = parseInt(e.target.dataset.id, 10);
    const list = e.target.dataset.list;
    if (list === "steam") {
      if (e.target.checked) completed.add(id); else completed.delete(id);
      renderSteam(); await invoke("save_completed", { completed: [...completed] });
    } else if (list === "nonsteam") {
      if (e.target.checked) completedNS.add(id); else completedNS.delete(id);
      renderNonSteam(); await invoke("save_completed_nonsteam", { completed: [...completedNS] });
    }
  }
});
document.getElementById("search").addEventListener("input", e => { q = e.target.value.toLowerCase().trim(); renderSteam(); });
document.getElementById("lNav").addEventListener("click", e => { if (e.target.tagName === "A") { e.preventDefault(); const t = document.querySelector(e.target.getAttribute("href")); if (t) t.scrollIntoView({ behavior: "smooth", block: "start" }); } });
document.getElementById("content").addEventListener("click", e => {
  if (e.target.type === "checkbox") return;
  const tr = e.target.closest("tr");
  if (!tr) return;
  const cb = tr.querySelector("input[data-id][data-list='steam']");
  if (!cb) return;
  openDetailPanel(parseInt(cb.dataset.id, 10));
});

getVersion().then(v => { document.getElementById("appVersion").textContent = "v" + v; });
init();
