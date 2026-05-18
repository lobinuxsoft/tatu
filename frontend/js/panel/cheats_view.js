import { invoke } from "../tauri.js";
import { state } from "../state.js";
import { esc } from "../utils.js";

function gameNameFor(gameId) {
  const fromSteam = state.G?.find(g => String(g.id) === String(gameId));
  if (fromSteam?.name) return fromSteam.name;
  const fromNonSteam = state.NS?.find(g => String(g.id) === String(gameId));
  return fromNonSteam?.name || "";
}

function fmtKB(bytes) {
  if (!Number.isFinite(bytes)) return "";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function renderCeBanner(status) {
  const kind = status?.kind;
  if (kind === "installed") {
    return `<div class="ce-banner ce-banner-ok">CE Linux ${esc(status.version)} installed</div>`;
  }
  if (kind === "corrupt") {
    return `<div class="ce-banner ce-banner-err">CE install corrupt: ${esc(status.reason || "")} <button class="ce-install-btn" data-action="install">Reinstall</button></div>`;
  }
  return `<div class="ce-banner ce-banner-warn">CE Linux not installed <button class="ce-install-btn" data-action="install">Install CE 7.6.6</button></div>`;
}

function renderTablesSection(gameId, tables) {
  if (!tables.length) {
    return (
      `<div class="ce-tables-section">` +
        `<div class="ce-tables-header">Available .CT tables` +
          `<button class="ce-refresh-btn" data-action="refresh-tables">Refresh</button>` +
        `</div>` +
        `<div class="ach-empty">` +
          `No .CT files in <code>~/.config/backlog-tracker/cheat-tables/${esc(String(gameId))}/</code>` +
        `</div>` +
      `</div>`
    );
  }
  let html =
    `<div class="ce-tables-section">` +
      `<div class="ce-tables-header">Available .CT tables` +
        `<button class="ce-refresh-btn" data-action="refresh-tables">Refresh</button>` +
      `</div>` +
      `<ul class="ce-tables-list">`;
  for (const t of tables) {
    html +=
      `<li class="ce-table-row">` +
        `<div class="ce-table-info">` +
          `<div class="ce-table-name">${esc(t.name)}</div>` +
          `<div class="ce-table-meta">${esc(fmtKB(t.size_bytes))}</div>` +
        `</div>` +
        `<button class="ce-open-btn" data-table="${esc(t.name)}">Open CE</button>` +
      `</li>`;
  }
  html += `</ul></div>`;
  return html;
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

function wireBanner(panel, gameId) {
  const btn = panel.querySelector(".ce-install-btn");
  if (!btn) return;
  btn.addEventListener("click", async () => {
    btn.disabled = true;
    const original = btn.textContent;
    btn.textContent = "Installing...";
    try {
      await invoke("ce_install_trigger");
      await loadCheats(gameId);
    } catch (e) {
      btn.textContent = "Failed";
      btn.title = String(e);
      setTimeout(() => { btn.textContent = original; btn.disabled = false; btn.removeAttribute("title"); }, 2500);
    }
  });
}

function wireTables(panel, gameId) {
  const refresh = panel.querySelector(".ce-refresh-btn");
  if (refresh) {
    refresh.addEventListener("click", () => loadCheats(gameId));
  }
  panel.querySelectorAll(".ce-open-btn").forEach(btn => {
    btn.addEventListener("click", async () => {
      const tableName = btn.dataset.table;
      const original = btn.textContent;
      btn.disabled = true;
      btn.textContent = "Opening...";
      try {
        await invoke("ce_open_for_game", { appId: String(gameId), tableName });
        btn.textContent = "✓ Launched";
        setTimeout(() => { btn.textContent = original; btn.disabled = false; }, 1500);
      } catch (e) {
        btn.textContent = "✗ Error";
        btn.title = String(e);
        setTimeout(() => { btn.textContent = original; btn.disabled = false; btn.removeAttribute("title"); }, 3000);
      }
    });
  });
}

function renderRuntimeSection(features) {
  if (!features.length) {
    return (
      `<div class="cheat-runtime-section">` +
        `<div class="cheat-runtime-header">Trainer features</div>` +
        `<div class="ach-empty">` +
          `No trainer manifests for this game.<br>` +
          `<small style="color:var(--fg-muted);display:block;margin-top:0.5rem">` +
            `Place a JSON manifest under <code>~/.config/backlog-tracker/trainers/&lt;appid&gt;/</code>` +
          `</small>` +
        `</div>` +
      `</div>`
    );
  }
  const anyRunning = features.some(f => f.game_running);
  const pillCls = anyRunning ? "cheat-pill-on" : "cheat-pill-off";
  const pillTxt = anyRunning ? "\u{1F7E2} Game running" : "\u{1F534} Game not running";

  let html =
    `<div class="cheat-runtime-section">` +
      `<div class="cheat-runtime-header">` +
        `Trainer features <span class="cheat-pill ${pillCls}">${pillTxt}</span>` +
      `</div>` +
      `<ul class="cheat-runtime-list">`;
  for (const f of features) {
    const dis = f.game_running ? "" : "disabled";
    const ch = f.active ? "checked" : "";
    const cat = f.category ? `${esc(f.category)} • ` : "";
    html +=
      `<li class="cheat-runtime-item">` +
        `<div class="cheat-runtime-info">` +
          `<div class="cheat-runtime-name">${esc(f.name)}</div>` +
          `<div class="cheat-runtime-meta">${cat}${esc(f.manifest_exe)}</div>` +
        `</div>` +
        `<label class="cheat-switch" title="${dis ? 'Launch the game first' : 'Toggle'}">` +
          `<input type="checkbox" data-feature-uuid="${esc(f.uuid)}" ${ch} ${dis}>` +
          `<span class="cheat-switch-slider"></span>` +
        `</label>` +
      `</li>`;
  }
  html += `</ul></div>`;
  return html;
}

function wireRuntimeSwitches(panel, gameId) {
  panel.querySelectorAll(".cheat-runtime-item input").forEach(input => {
    input.addEventListener("change", async () => {
      const uuid = input.dataset.featureUuid;
      const desired = input.checked;
      input.disabled = true;
      try {
        if (desired) {
          await invoke("cheat_runtime_enable", { appId: String(gameId), featureUuid: uuid });
        } else {
          await invoke("cheat_runtime_disable", { featureUuid: uuid });
        }
      } catch (e) {
        input.checked = !desired;
        input.title = String(e);
        setTimeout(() => input.removeAttribute("title"), 3000);
      } finally {
        input.disabled = false;
      }
    });
  });
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
    const [ceStatus, tables, runtimeFeatures, status, list] = await Promise.all([
      invoke("ce_install_status").catch(() => ({ kind: "not_installed" })),
      invoke("ce_list_tables_for_game", { appId: String(gameId) }).catch(() => []),
      invoke("cheat_runtime_list_features", { appId: String(gameId) }).catch(() => []),
      invoke("cheat_status", { appId: gameId }),
      invoke("cheat_list", { appId: gameId }).catch(() => null),
    ]);

    if (state.panelGameId !== gameId) return;

    const banner = renderCeBanner(ceStatus);
    const runtimeSection = renderRuntimeSection(runtimeFeatures);
    const tablesSection = renderTablesSection(gameId, tables);
    const searchBar = renderSearchBar(gameId);

    let nativeBlock = "";
    // Legacy cheat-core section is deprecated: only render when the user has
    // pre-existing data on disk. Empty state is hidden entirely so the
    // surrounding UI doesn't keep recommending the obsolete format.
    if (status.has_cheats && list) {
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
      nativeBlock =
        `<div class="cheat-legacy-header">` +
          `Legacy native cheats <span class="cheat-legacy-tag">deprecated</span>` +
        `</div>` +
        `<div class="cheat-legacy-note">` +
          `Reads <code>~/.config/backlog-tracker/cheats/${gameId}.json</code>. ` +
          `Will be auto-migrated to the manifest format above in a future update.` +
        `</div>` +
        `<div class="cheat-status-bar"><span class="cheat-pill ${pillCls}">${pillTxt}</span></div>` +
        `<ul class="cheat-list">`;
      for (const c of list) {
        const desc = c.description ? `<div class="cheat-desc">${esc(c.description)}</div>` : "";
        const dis = status.process_running ? "" : "disabled";
        const control = c.action_kind === "Freeze"
          ? renderFreezeSwitch(c.id, frozen.get(c.id) === true, dis)
          : renderTriggerButton(c.id, dis);
        nativeBlock +=
          `<li class="cheat-item">` +
            `<div class="cheat-info">` +
              `<div class="cheat-name">${esc(c.name)} <span class="cheat-type">${esc(c.value_type)}</span></div>` +
              desc +
            `</div>` +
            control +
          `</li>`;
      }
      nativeBlock += `</ul>`;
    }

    panel.innerHTML = banner + runtimeSection + tablesSection + searchBar + nativeBlock;

    wireBanner(panel, gameId);
    wireRuntimeSwitches(panel, gameId);
    wireTables(panel, gameId);
    wireSearchButton(panel);

    panel.querySelectorAll(".cheat-trigger-btn").forEach(btn => {
      btn.addEventListener("click", () => triggerCheat(gameId, btn.dataset.cheatId, btn));
    });
    panel.querySelectorAll(".cheat-list .cheat-switch input").forEach(input => {
      input.addEventListener("change", () => toggleFreeze(gameId, input.dataset.cheatId, input));
    });
  } catch (e) {
    if (state.panelGameId !== gameId) return;
    panel.innerHTML = `<div class="ach-empty">Error loading cheats: ${esc(String(e))}</div>`;
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
