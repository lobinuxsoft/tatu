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

// Render the Tatu Launcher backend banner: install state of the Steam
// compat tool drop-in + per-game backend toggle + Proton picker. Sits
// above the CE banner because "is the Bridge backend even available
// for this game" is a precondition for everything else the cheats
// panel surfaces.
function renderTatuBanner(launcherStatus, currentBackend, launcherGame, protons) {
  const kind = launcherStatus?.kind;
  const usingBridge = currentBackend?.kind === "bridge";

  if (kind === "not_installed") {
    return (
      `<div class="ce-banner ce-banner-warn tatu-banner">` +
        `<span>Tatu Launcher backend not installed — bridge attach unavailable for Proton games.</span>` +
        `<button class="ce-install-btn tatu-install-btn" data-action="tatu-install">Install Tatu</button>` +
      `</div>`
    );
  }

  if (kind === "corrupt") {
    return (
      `<div class="ce-banner ce-banner-err tatu-banner">` +
        `<span>Tatu Launcher install corrupt: ${esc(launcherStatus.reason || "")}</span>` +
        `<button class="ce-install-btn tatu-install-btn" data-action="tatu-install">Reinstall</button>` +
      `</div>`
    );
  }

  // Installed. Surface the per-game toggle + Proton picker.
  const version = esc(launcherStatus.version || "?");
  const pillTxt = usingBridge ? `Tatu Launcher (bridge)` : `Linux ptrace`;
  const pillCls = usingBridge ? "tatu-pill-on" : "tatu-pill-off";
  const btnLabel = usingBridge ? "Revert to Linux" : "Switch to Tatu";
  const btnAction = usingBridge ? "tatu-disable" : "tatu-enable";

  // Render the Proton picker even when the toggle is off so the user
  // can pre-pick before flipping the switch. Disabled state is purely
  // visual feedback that the choice won't be honoured until Switch.
  const protonOpts = (protons || []).map(p => {
    const sel = p.name === launcherGame?.proton ? " selected" : "";
    return `<option value="${esc(p.name)}"${sel}>${esc(p.name)} (${esc(p.kind)})</option>`;
  }).join("");
  const protonSelector = launcherGame
    ? `<label class="tatu-proton-label">Proton: ` +
        `<select class="tatu-proton-select" ${usingBridge ? "" : "disabled"}>` +
          protonOpts +
        `</select>` +
      `</label>`
    : "";

  return (
    `<div class="ce-banner ce-banner-ok tatu-banner">` +
      `<span class="tatu-backend-label">Cheat backend:</span>` +
      `<span class="tatu-backend-pill ${pillCls}">${esc(pillTxt)}</span>` +
      protonSelector +
      `<span class="tatu-version">Tatu ${version}</span>` +
      `<button class="ce-install-btn tatu-toggle-btn" data-action="${btnAction}">${esc(btnLabel)}</button>` +
    `</div>`
  );
}

function wireTatuBanner(panel, gameId, launcherGame) {
  panel.querySelectorAll(".tatu-install-btn, .tatu-toggle-btn").forEach(btn => {
    btn.addEventListener("click", async () => {
      const action = btn.dataset.action;
      btn.disabled = true;
      const original = btn.textContent;
      btn.textContent = "Working…";
      try {
        if (action === "tatu-install") {
          await invoke("tatu_launcher_install");
        } else if (action === "tatu-enable") {
          // Patch order: config.vdf so Steam picks the drop-in on
          // next launch, launcher.toml so the drop-in actually swaps
          // to the bridge for this appid (without this the launcher
          // passes through — Phase 7C bug), then state.json so the
          // tracker routes value cheats through the bridge.
          const select = panel.querySelector(".tatu-proton-select");
          const proton = select?.value || launcherGame?.proton || "";
          await invoke("tatu_launcher_set_for_app", { appId: String(gameId) });
          await invoke("launcher_config_set_for_app", {
            appId: String(gameId),
            view: { proton, target_exe: launcherGame?.target_exe || "", tatu_enabled: true },
          });
          const choice = await invoke("cheat_runtime_backend_recommend", { appId: String(gameId) });
          await invoke("cheat_runtime_backend_set", { appId: String(gameId), backend: choice });
        } else if (action === "tatu-disable") {
          // Drop the launcher.toml entry too so the launcher reverts
          // to passthrough — without this the bridge would keep
          // hooking the game on the next launch even though the
          // tracker is no longer routing through it.
          await invoke("launcher_config_unset_app", { appId: String(gameId) });
          await invoke("cheat_runtime_backend_set", {
            appId: String(gameId),
            backend: { kind: "linux" },
          });
        }
        await loadCheats(gameId);
      } catch (e) {
        btn.textContent = "✗ " + (String(e).split("\n")[0] || "Error");
        btn.title = String(e);
        setTimeout(() => {
          btn.textContent = original;
          btn.disabled = false;
          btn.removeAttribute("title");
        }, 4000);
      }
    });
  });

  // Proton dropdown — only meaningful when tatu_enabled is on, but
  // we still persist the choice so flipping the toggle later uses
  // it. Saving is fire-and-forget; failures surface as a brief
  // border flash on the select.
  const select = panel.querySelector(".tatu-proton-select");
  if (select && launcherGame) {
    select.addEventListener("change", async () => {
      const next = {
        proton: select.value,
        target_exe: launcherGame.target_exe || "",
        tatu_enabled: launcherGame.tatu_enabled,
      };
      select.disabled = true;
      try {
        await invoke("launcher_config_set_for_app", { appId: String(gameId), view: next });
        select.classList.add("tatu-proton-saved");
        setTimeout(() => select.classList.remove("tatu-proton-saved"), 800);
      } catch (e) {
        select.classList.add("tatu-proton-error");
        select.title = String(e);
        setTimeout(() => {
          select.classList.remove("tatu-proton-error");
          select.removeAttribute("title");
        }, 3000);
      } finally {
        select.disabled = false;
      }
    });
  }
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
    if (f.kind === "header") {
      // Visual-only divider: no switch, no toggle. Renders as a small caps
      // title above the next batch of features. Mirrors CE's `<GroupHeader>`
      // entries — see MemoryRecordUnit.pas:148.
      html +=
        `<li class="cheat-runtime-header-row">` +
          `<div class="cheat-runtime-header-text">${esc(f.name)}</div>` +
        `</li>`;
      continue;
    }
    if (f.kind === "value") {
      html += renderValueRow(f);
      continue;
    }
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

/// Render one Value-kind row: typed numeric input + Set + Freeze. Dimmed
/// (with reason) when the game isn't running OR the master scaffold cheat
/// hasn't been enabled — the backend flags both via `game_running` and
/// `symbol_ready` in the FeatureView.
function renderValueRow(f) {
  const cat = f.category ? `${esc(f.category)} • ` : "";
  const spec = f.value_spec || {};
  const vt = spec.vtype || "u32";
  const offsets = (spec.offsets || []).map(o => "0x" + o.toString(16).toUpperCase()).join(",");
  const addrSummary = `${esc(spec.base_expr || "?")}${offsets ? " + [" + esc(offsets) + "]" : ""}`;
  const blockedReason = !f.game_running
    ? "Launch the game first"
    : !f.symbol_ready
    ? `Enable the scaffold cheat that registers '${esc(f.required_symbol || "?")}' first`
    : "";
  const blocked = blockedReason !== "";
  const dis = blocked ? "disabled" : "";
  const blockedAttr = blocked ? `title="${blockedReason}"` : "";
  const isFloat = vt === "f32" || vt === "f64";
  const step = isFloat ? "any" : "1";
  return (
    `<li class="cheat-runtime-item cheat-runtime-value ${blocked ? 'cheat-runtime-value-blocked' : ''}" data-feature-uuid="${esc(f.uuid)}" data-vtype="${esc(vt)}">` +
      `<div class="cheat-runtime-info">` +
        `<div class="cheat-runtime-name">${esc(f.name)} <span class="cheat-runtime-vtype">${esc(vt)}</span></div>` +
        `<div class="cheat-runtime-meta">${cat}${addrSummary}</div>` +
      `</div>` +
      `<div class="cheat-runtime-value-controls" ${blockedAttr}>` +
        `<button class="cheat-value-read" data-action="read" ${dis}>↻</button>` +
        `<input class="cheat-value-input" type="number" step="${step}" value="" ${dis}>` +
        `<button class="cheat-value-set" data-action="set" ${dis}>Set</button>` +
        `<label class="cheat-switch cheat-switch-sm" title="${dis ? blockedReason : 'Freeze at current input value'}">` +
          `<input type="checkbox" class="cheat-value-freeze" data-action="freeze" ${dis}>` +
          `<span class="cheat-switch-slider"></span>` +
        `</label>` +
      `</div>` +
    `</li>`
  );
}

function parseValueByType(text, vtype) {
  if (text === "" || text === null) return null;
  if (vtype === "f32" || vtype === "f64") {
    const f = Number(text);
    return Number.isFinite(f) ? { vtype, value: f } : null;
  }
  // Integer parse, base-10. Backend serializes signed types as JSON numbers.
  const n = Number(text);
  if (!Number.isFinite(n) || !Number.isInteger(n)) return null;
  return { vtype, value: n };
}

function formatValuePayload(payload) {
  if (!payload || payload.value === undefined) return "";
  return String(payload.value);
}

function wireValueRows(panel, gameId) {
  panel.querySelectorAll(".cheat-runtime-value").forEach(row => {
    const uuid = row.dataset.featureUuid;
    const vtype = row.dataset.vtype;
    const input = row.querySelector(".cheat-value-input");
    const readBtn = row.querySelector(".cheat-value-read");
    const setBtn = row.querySelector(".cheat-value-set");
    const freezeCb = row.querySelector(".cheat-value-freeze");

    const flash = (el, ok) => {
      el.classList.toggle("cheat-value-flash-ok", ok);
      el.classList.toggle("cheat-value-flash-err", !ok);
      setTimeout(() => {
        el.classList.remove("cheat-value-flash-ok", "cheat-value-flash-err");
      }, 800);
    };

    if (readBtn) {
      readBtn.addEventListener("click", async () => {
        readBtn.disabled = true;
        try {
          const payload = await invoke("cheat_runtime_value_read", {
            appId: String(gameId),
            featureUuid: uuid,
          });
          input.value = formatValuePayload(payload);
          flash(input, true);
        } catch (e) {
          input.title = String(e);
          flash(input, false);
          setTimeout(() => input.removeAttribute("title"), 3000);
        } finally {
          readBtn.disabled = false;
        }
      });
    }

    if (setBtn) {
      setBtn.addEventListener("click", async () => {
        const value = parseValueByType(input.value, vtype);
        if (!value) {
          flash(input, false);
          return;
        }
        setBtn.disabled = true;
        try {
          await invoke("cheat_runtime_value_write", {
            appId: String(gameId),
            featureUuid: uuid,
            value,
          });
          flash(setBtn, true);
        } catch (e) {
          setBtn.title = String(e);
          flash(setBtn, false);
          setTimeout(() => setBtn.removeAttribute("title"), 3000);
        } finally {
          setBtn.disabled = false;
        }
      });
    }

    if (freezeCb) {
      freezeCb.addEventListener("change", async () => {
        const enabled = freezeCb.checked;
        const value = enabled ? parseValueByType(input.value, vtype) : null;
        if (enabled && !value) {
          freezeCb.checked = false;
          flash(input, false);
          return;
        }
        freezeCb.disabled = true;
        try {
          await invoke("cheat_runtime_value_freeze", {
            req: {
              app_id: String(gameId),
              feature_uuid: uuid,
              enabled,
              value,
            },
          });
        } catch (e) {
          freezeCb.checked = !enabled;
          freezeCb.title = String(e);
          setTimeout(() => freezeCb.removeAttribute("title"), 3000);
        } finally {
          freezeCb.disabled = false;
        }
      });
    }
  });
}

function wireRuntimeSwitches(panel, gameId) {
  // Limit to Toggle rows — Value rows have their own `.cheat-runtime-value`
  // container and must not be wired as plain enable/disable switches.
  panel.querySelectorAll(".cheat-runtime-item:not(.cheat-runtime-value) input[data-feature-uuid]").forEach(input => {
    input.addEventListener("change", async () => {
      const uuid = input.dataset.featureUuid;
      const desired = input.checked;
      input.disabled = true;
      try {
        if (desired) {
          await invoke("cheat_runtime_enable", { appId: String(gameId), featureUuid: uuid });
        } else {
          await invoke("cheat_runtime_disable", { appId: String(gameId), featureUuid: uuid });
        }
        // Master toggles register / unregister symbols that gate Value rows.
        // Refresh so symbol_ready reflects the new state.
        loadCheats(gameId);
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

function renderOrphansBanner(orphans, gameId) {
  if (!orphans || !orphans.length) return "";
  const ours = orphans.filter(o => String(o.app_id) === String(gameId));
  if (!ours.length) return "";
  const itemsHtml = ours
    .map(
      o =>
        `<li class="orphans-item" data-uuid="${esc(o.feature_uuid)}">` +
          `<div class="orphans-name">${esc(o.name || o.feature_uuid)}</div>` +
          `<div class="orphans-meta">pid ${o.pid} • ${o.writes} writes • ${o.allocs} allocs</div>` +
          `<button class="orphans-restore" data-uuid="${esc(o.feature_uuid)}">Restore</button>` +
          `<button class="orphans-dismiss" data-uuid="${esc(o.feature_uuid)}">Dismiss</button>` +
        `</li>`
    )
    .join("");
  return (
    `<div class="orphans-banner">` +
      `<div class="orphans-title">⚠ Orphan hooks from a previous session</div>` +
      `<div class="orphans-help">` +
        `The tracker exited before these hooks were disabled. Restore reverts the trampoline ` +
        `bytes back to the original code; Dismiss only drops the record (use this if you've ` +
        `already re-launched the game and the bytes are fresh anyway).` +
      `</div>` +
      `<ul class="orphans-list">${itemsHtml}</ul>` +
    `</div>`
  );
}

function wireOrphansBanner(panel, gameId) {
  panel.querySelectorAll(".orphans-restore").forEach(btn => {
    btn.addEventListener("click", async () => {
      const uuid = btn.dataset.uuid;
      btn.disabled = true;
      const original = btn.textContent;
      btn.textContent = "Restoring…";
      try {
        await invoke("cheat_runtime_orphans_restore", {
          appId: String(gameId),
          featureUuid: uuid,
        });
        loadCheats(gameId);
      } catch (e) {
        btn.textContent = "✗ Error";
        btn.title = String(e);
        setTimeout(() => {
          btn.textContent = original;
          btn.disabled = false;
          btn.removeAttribute("title");
        }, 3000);
      }
    });
  });
  panel.querySelectorAll(".orphans-dismiss").forEach(btn => {
    btn.addEventListener("click", async () => {
      const uuid = btn.dataset.uuid;
      btn.disabled = true;
      try {
        await invoke("cheat_runtime_orphans_dismiss", {
          appId: String(gameId),
          featureUuid: uuid,
        });
        loadCheats(gameId);
      } catch (e) {
        btn.title = String(e);
        setTimeout(() => btn.removeAttribute("title"), 3000);
      } finally {
        btn.disabled = false;
      }
    });
  });
}

export async function loadCheats(gameId) {
  const panel = document.getElementById("dpCheats");
  if (!panel) return;

  try {
    const [ceStatus, tables, runtimeFeatures, orphans, tatuStatus, currentBackend, launcherGame, protons] = await Promise.all([
      invoke("ce_install_status").catch(() => ({ kind: "not_installed" })),
      invoke("ce_list_tables_for_game", { appId: String(gameId) }).catch(() => []),
      invoke("cheat_runtime_list_features", { appId: String(gameId) }).catch(() => []),
      invoke("cheat_runtime_orphans_list").catch(() => []),
      invoke("tatu_launcher_status").catch(() => ({ kind: "not_installed" })),
      invoke("cheat_runtime_backend_get", { appId: String(gameId) }).catch(() => ({ kind: "linux" })),
      invoke("launcher_config_get_for_app", { appId: String(gameId) }).catch(() => null),
      invoke("launcher_list_protons").catch(() => []),
    ]);

    if (state.panelGameId !== gameId) return;

    const tatuBanner = renderTatuBanner(tatuStatus, currentBackend, launcherGame, protons);
    const banner = renderCeBanner(ceStatus);
    const runtimeSection = renderRuntimeSection(runtimeFeatures);
    const tablesSection = renderTablesSection(gameId, tables);
    const searchBar = renderSearchBar(gameId);

    const orphansBanner = renderOrphansBanner(orphans, gameId);
    panel.innerHTML = tatuBanner + banner + orphansBanner + runtimeSection + tablesSection + searchBar;

    wireTatuBanner(panel, gameId, launcherGame);
    wireBanner(panel, gameId);
    wireOrphansBanner(panel, gameId);
    wireRuntimeSwitches(panel, gameId);
    wireValueRows(panel, gameId);
    wireTables(panel, gameId);
    wireSearchButton(panel);
  } catch (e) {
    if (state.panelGameId !== gameId) return;
    panel.innerHTML = `<div class="ach-empty">Error loading cheats: ${esc(String(e))}</div>`;
  }
}
