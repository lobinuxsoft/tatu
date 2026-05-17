import { invoke } from "../tauri.js";
import { state } from "../state.js";
import { esc } from "../utils.js";

function gameNameFor(gameId) {
  const fromSteam = state.G?.find(g => String(g.id) === String(gameId));
  if (fromSteam?.name) return fromSteam.name;
  const fromNonSteam = state.NS?.find(g => String(g.id) === String(gameId));
  return fromNonSteam?.name || "";
}

function renderSearchBar(gameId) {
  const name = gameNameFor(gameId);
  if (!name) return "";
  return (
    `<div class="cheat-search-bar">` +
      `<button class="cheat-search-btn" data-game-name="${esc(name)}" title="Open Fearless Revolution search in your browser">` +
        `Search Fearless Revolution` +
      `</button>` +
    `</div>`
  );
}

function wireSearchButton(panel) {
  const btn = panel.querySelector(".cheat-search-btn");
  if (!btn) return;
  btn.addEventListener("click", async () => {
    btn.disabled = true;
    try {
      await invoke("open_fearless_search", { gameName: btn.dataset.gameName });
    } catch (e) {
      btn.title = String(e);
      setTimeout(() => btn.removeAttribute("title"), 2500);
    } finally {
      btn.disabled = false;
    }
  });
}

export async function loadCheats(gameId) {
  const panel = document.getElementById("dpCheats");
  if (!panel) return;

  try {
    const [status, list] = await Promise.all([
      invoke("cheat_status", { appId: gameId }),
      invoke("cheat_list", { appId: gameId }).catch(() => null),
    ]);

    if (state.panelGameId !== gameId) return;

    const searchBar = renderSearchBar(gameId);

    if (!status.has_cheats || !list) {
      panel.innerHTML =
        searchBar +
        `<div class="ach-empty">` +
          `No hay cheats configurados para este juego.<br>` +
          `<small style="color:var(--fg-muted);display:block;margin-top:0.5rem">` +
            `Crear archivo <code>~/.config/backlog-tracker/cheats/${gameId}.json</code>` +
          `</small>` +
        `</div>`;
      wireSearchButton(panel);
      return;
    }

    const freezeIds = list.filter(c => c.action_kind === "Freeze").map(c => c.id);
    const freezeStates = await Promise.all(
      freezeIds.map(id =>
        invoke("cheat_freeze_status", { appId: gameId, cheatId: id }).catch(() => false)
      )
    );
    if (state.panelGameId !== gameId) return;
    const frozen = new Map(freezeIds.map((id, i) => [id, freezeStates[i]]));

    const pillCls = status.process_running ? "cheat-pill-on" : "cheat-pill-off";
    const pillTxt = status.process_running ? "\u{1F7E2} Game running" : "\u{1F534} Game not running";
    let html = searchBar + `<div class="cheat-status-bar"><span class="cheat-pill ${pillCls}">${pillTxt}</span></div>`;
    html += `<ul class="cheat-list">`;
    for (const c of list) {
      const desc = c.description ? `<div class="cheat-desc">${esc(c.description)}</div>` : "";
      const dis = status.process_running ? "" : "disabled";
      const control = c.action_kind === "Freeze"
        ? renderFreezeSwitch(c.id, frozen.get(c.id) === true, dis)
        : renderTriggerButton(c.id, dis);
      html +=
        `<li class="cheat-item">` +
          `<div class="cheat-info">` +
            `<div class="cheat-name">${esc(c.name)} <span class="cheat-type">${esc(c.value_type)}</span></div>` +
            desc +
          `</div>` +
          control +
        `</li>`;
    }
    html += `</ul>`;
    panel.innerHTML = html;

    panel.querySelectorAll(".cheat-trigger-btn").forEach(btn => {
      btn.addEventListener("click", () => triggerCheat(gameId, btn.dataset.cheatId, btn));
    });
    panel.querySelectorAll(".cheat-switch input").forEach(input => {
      input.addEventListener("change", () => toggleFreeze(gameId, input.dataset.cheatId, input));
    });
    wireSearchButton(panel);
  } catch (e) {
    if (state.panelGameId !== gameId) return;
    panel.innerHTML = `<div class="ach-empty">Error al cargar cheats: ${esc(String(e))}</div>`;
  }
}

function renderTriggerButton(cheatId, dis) {
  return `<button class="cheat-trigger-btn" data-cheat-id="${esc(cheatId)}" ${dis}>Trigger</button>`;
}

function renderFreezeSwitch(cheatId, checked, dis) {
  const ch = checked ? "checked" : "";
  return (
    `<label class="cheat-switch" title="Freeze">` +
      `<input type="checkbox" data-cheat-id="${esc(cheatId)}" ${ch} ${dis}>` +
      `<span class="cheat-switch-slider"></span>` +
    `</label>`
  );
}

async function triggerCheat(gameId, cheatId, btn) {
  const originalText = btn.textContent;
  btn.disabled = true;
  btn.textContent = "...";
  try {
    await invoke("cheat_trigger", { appId: gameId, cheatId });
    btn.textContent = "✓ OK";
    setTimeout(() => {
      btn.textContent = originalText;
      btn.disabled = false;
    }, 1200);
  } catch (e) {
    btn.textContent = "✗ Error";
    btn.title = String(e);
    setTimeout(() => {
      btn.textContent = originalText;
      btn.disabled = false;
      btn.removeAttribute("title");
    }, 2500);
  }
}

async function toggleFreeze(gameId, cheatId, input) {
  const desired = input.checked;
  input.disabled = true;
  try {
    const actual = await invoke("cheat_freeze_toggle", {
      appId: gameId,
      cheatId,
      enabled: desired,
    });
    input.checked = actual === true;
  } catch (e) {
    input.checked = !desired;
    input.title = String(e);
    setTimeout(() => input.removeAttribute("title"), 2500);
  } finally {
    input.disabled = false;
  }
}
