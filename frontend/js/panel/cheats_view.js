import { invoke } from "../tauri.js";
import { state } from "../state.js";
import { esc } from "../utils.js";

export async function loadCheats(gameId) {
  const panel = document.getElementById("dpCheats");
  if (!panel) return;

  try {
    const [status, list] = await Promise.all([
      invoke("cheat_status", { appId: gameId }),
      invoke("cheat_list", { appId: gameId }).catch(() => null),
    ]);

    if (state.panelGameId !== gameId) return;

    if (!status.has_cheats || !list) {
      panel.innerHTML =
        `<div class="ach-empty">` +
          `No hay cheats configurados para este juego.<br>` +
          `<small style="color:var(--fg-muted);display:block;margin-top:0.5rem">` +
            `Crear archivo <code>~/.config/backlog-tracker/cheats/${gameId}.json</code>` +
          `</small>` +
        `</div>`;
      return;
    }

    const pillCls = status.process_running ? "cheat-pill-on" : "cheat-pill-off";
    const pillTxt = status.process_running ? "\u{1F7E2} Game running" : "\u{1F534} Game not running";
    let html = `<div class="cheat-status-bar"><span class="cheat-pill ${pillCls}">${pillTxt}</span></div>`;
    html += `<ul class="cheat-list">`;
    for (const c of list) {
      const desc = c.description ? `<div class="cheat-desc">${esc(c.description)}</div>` : "";
      const dis = status.process_running ? "" : "disabled";
      html +=
        `<li class="cheat-item">` +
          `<div class="cheat-info">` +
            `<div class="cheat-name">${esc(c.name)} <span class="cheat-type">${esc(c.value_type)}</span></div>` +
            desc +
          `</div>` +
          `<button class="cheat-trigger-btn" data-cheat-id="${esc(c.id)}" ${dis}>Trigger</button>` +
        `</li>`;
    }
    html += `</ul>`;
    panel.innerHTML = html;

    panel.querySelectorAll(".cheat-trigger-btn").forEach(btn => {
      btn.addEventListener("click", () => triggerCheat(gameId, btn.dataset.cheatId, btn));
    });
  } catch (e) {
    if (state.panelGameId !== gameId) return;
    panel.innerHTML = `<div class="ach-empty">Error al cargar cheats: ${esc(String(e))}</div>`;
  }
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
