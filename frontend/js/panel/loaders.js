import { invoke } from "../tauri.js";
import { state, TL, TC } from "../state.js";
import { esc, fmtDate, mcColor } from "../utils.js";
import { doneLoading, startLoading } from "../loading.js";
import { renderSteam } from "../render/steam.js";
import { emit } from "../tauri.js";
import { renderDrmBadge, renderDrmExplanation, renderPreservabilityBlock } from "./drm_view.js";
import { openCartridgeModal } from "../modals/cartridge.js";
import { headerImg, infoRow, infoRowInner, tagsRow } from "./detail_template.js";


// Caches (DRM, HowLongToBeat, achievements) are refreshed from whichever
// window the user is looking at. Re-render locally, and tell the other window
// so the list behind the detail view does not go stale.
function refreshLibraryViews() {
  renderSteam();
  emit("library-updated").catch(() => {});
}

export async function loadGameDetails(gameId) {
  try {
    const g = await invoke("get_game_details", { appId: gameId });
    if (state.panelGameId !== gameId) return;

    const idx = state.G.findIndex(x => x.id === gameId);
    if (idx >= 0) state.G[idx] = { ...state.G[idx], ...g };

    const imgEl = document.getElementById("dpHeaderImg");
    if (imgEl && g.header_img) imgEl.innerHTML = headerImg(g.header_img);

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

    html += tagsRow("Generos", g.genres);
    if (g.developers && g.developers.length) {
      html += infoRow("Developer", esc(g.developers.join(", ")));
    }
    if (g.has_cards) {
      html += infoRow("Cromos", `<span class="tag tag-cards">\u{1F0CF} Tiene cromos de Steam</span>`);
    }
    if (g.metacritic) {
      html += infoRow("Metacritic", `<span class="metacritic-badge ${mcColor(g.metacritic)}">${g.metacritic}</span>`);
    }
    if (g.tag) {
      html += infoRow("Categoria", `<span class="tag ${TC[g.tag]}">${TL[g.tag]}</span>`);
    }
    html += infoRow("Horas", g.hours > 0 ? g.hours + "h" : "Sin jugar");
    html += infoRow("Duracion (HLTB)", `<span style="color:#6e7681">Buscando...</span>`, "dpHltb");
    html += infoRow("DRM", `<span style="color:#6e7681">Consultando...</span>`, "dpDrm");

    if (!html) html = `<div class="ach-empty">No hay detalles disponibles.</div>`;
    html = `<div class="detail-info-row"><button class="cartridge-btn" id="cartridgeBtn">💾 Instalar en cartucho</button></div>` + html;
    infoEl.innerHTML = html;
    document.getElementById("cartridgeBtn").onclick = () => openCartridgeModal(gameId);
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
    if (state.panelGameId !== gameId) return;
    const infoEl = document.getElementById("dpInfo");
    if (infoEl) infoEl.innerHTML = `<div class="ach-empty">Error al cargar info: ${esc(String(e))}</div>`;
  }
}

export async function loadAchievements(gameId) {
  const panel = document.getElementById("dpLogros");
  try {
    const data = await invoke("get_game_achievements", { appId: gameId });
    if (state.panelGameId !== gameId) return;

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
    state.achProgress[gameId] = [done, total];
    doneLoading("logros");
    refreshLibraryViews();
  } catch (e) {
    doneLoading("logros");
    if (state.panelGameId !== gameId) return;
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

export async function loadCards(gameId) {
  const panel = document.getElementById("dpCromos");
  if (!panel) return;
  try {
    const data = await invoke("get_game_cards", { appId: gameId });
    if (state.panelGameId !== gameId) return;

    const cards = data.cards || [];
    if (cards.length === 0) {
      panel.innerHTML = `<div class="ach-empty">No hay cromos disponibles para este juego.</div>`;
      return;
    }

    const owned = cards.filter(c => c.owned);
    const total = cards.length;
    const pct = total ? Math.round(owned.length / total * 100) : 0;

    let html = "";

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
    if (state.panelGameId !== gameId) return;
    const errStr = String(e);
    if (errStr.includes("No trading cards")) {
      if (panel) panel.innerHTML = `<div class="ach-empty">Este juego no tiene cromos.</div>`;
    } else {
      if (panel) panel.innerHTML = `<div class="ach-empty">Error al cargar cromos: ${esc(errStr)}</div>`;
    }
  }
}

export async function loadDrm(gameId) {
  try {
    const info = await invoke("get_game_drm", { appId: gameId });
    state.drmCache[gameId] = info;
    refreshLibraryViews();
    if (state.panelGameId !== gameId) return;
    const el = document.getElementById("dpDrm");
    if (!el) return;
    const game = state.G.find(x => x.id === gameId);
    const gameName = game ? game.name : "";
    const badge = renderDrmBadge(info);
    const explanation = renderDrmExplanation(info);
    const preservation = renderPreservabilityBlock(info, gameName);
    el.innerHTML = infoRowInner("DRM", `${badge}${explanation}${preservation}`, "div", "drm-detail-value");
  } catch (_) {
    const el = document.getElementById("dpDrm");
    if (el) {
      el.querySelector(".detail-info-value").textContent = "Error";
      el.querySelector(".detail-info-value").style.color = "#f85149";
    }
  }
}

export async function loadHltb(gameName, gameId) {
  try {
    const r = await invoke("search_hltb", { appId: gameId, gameName });
    if (state.panelGameId !== gameId) return;
    const el = document.getElementById("dpHltb");
    if (!el) return;
    if (!r) {
      el.querySelector(".detail-info-value").textContent = "No encontrado";
      el.querySelector(".detail-info-value").style.color = "#484f58";
      return;
    }
    state.hltbCache[gameId] = r;
    let html = '<div style="display:flex;gap:1rem;flex-wrap:wrap">';
    if (r.main_hours > 0) html += `<span class="tag tag-genre">Historia: ${r.main_hours}h</span>`;
    if (r.extra_hours > 0) html += `<span class="tag tag-ach">Main+Extra: ${r.extra_hours}h</span>`;
    if (r.completionist_hours > 0) html += `<span class="tag tag-cards">100%: ${r.completionist_hours}h</span>`;
    html += "</div>";
    el.innerHTML = infoRowInner("Duracion (HLTB)", html);
    refreshLibraryViews();
  } catch (_) {
    const el = document.getElementById("dpHltb");
    if (el) { el.querySelector(".detail-info-value").textContent = "Error"; el.querySelector(".detail-info-value").style.color = "#f85149"; }
  }
}
