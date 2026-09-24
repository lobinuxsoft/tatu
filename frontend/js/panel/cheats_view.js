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
// Wine bridge backend banner + handlers removed post-pivot #128.
// Tatu now runs every cheat through the native Linux ptrace runtime
// (cheat-runtime) directly — no backend selection, no compat tool,
// no per-game Proton picker. The cheats panel renders the runtime
// section and tables list with no banner above them.

function renderTablesSection(gameId, tables) {
  // Hidden file input lives outside the table list so it survives a
  // re-render mid-pick (loadCheats() rewrites .ce-tables-section but the
  // panel itself is stable). The visible button proxies the click.
  const importControls =
    `<button class="ce-import-btn" data-action="import-ct" title="Pick a .CT file to import into this game's library">Import .CT</button>` +
    `<input type="file" class="ce-import-input" accept=".ct,.CT" hidden>`;
  if (!tables.length) {
    return (
      `<div class="ce-tables-section">` +
        `<div class="ce-tables-header">Available .CT tables` +
          importControls +
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
        importControls +
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
        `<button class="ce-remove-btn" data-table="${esc(t.name)}" title="Delete this .CT and its manifest">✗</button>` +
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
  wireImportButton(panel, gameId);
  wireRemoveTableButtons(panel, gameId);
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

// #98 — surface REFramework / other prereqs above the runtime section.
// The backend returns one PrereqReport per distinct prereq declared by
// any manifest of this game; an empty array means nothing to install
// (the common case for vanilla UE / Mono / native titles).
function renderPrereqBanner(prereqs) {
  if (!Array.isArray(prereqs) || !prereqs.length) return "";
  const rows = [];
  for (const p of prereqs) {
    const label = prereqLabel(p.kind);
    if (p.status === "installed") {
      rows.push(
        `<div class="ce-banner ce-banner-ok">${esc(label)} installed at <code>${esc(p.game_dir)}</code></div>`
      );
      continue;
    }
    const reason = p.status === "corrupt"
      ? `${esc(label)} install looks corrupt — reinstall to recover`
      : `${esc(label)} required by this game's cheats`;
    const btnText = p.status === "corrupt" ? `Reinstall ${esc(label)}` : `Install ${esc(label)}`;
    rows.push(
      `<div class="ce-banner ce-banner-warn">` +
        `${reason} ` +
        `<button class="ce-prereq-install-btn" data-kind="${esc(p.kind)}">${btnText}</button>` +
      `</div>`
    );
  }
  return rows.join("");
}

function prereqLabel(kind) {
  if (kind === "reframework") return "REFramework";
  return kind;
}

// True iff at least one prereq is in `missing` or `corrupt` state — the
// runtime section dims its toggles in that case. The user can still
// expand the tree (so they see what the game offers) but cannot enable
// cheats until the prereq is in place.
function anyPrereqBlocking(prereqs) {
  if (!Array.isArray(prereqs)) return false;
  return prereqs.some(p => p.status === "missing" || p.status === "corrupt");
}

// Click handler for `Install REFramework` (and future prereq variants).
// The install command is synchronous on the backend (downloads + extracts
// + atomic rename within a single call) — the UI shows a spinner via the
// button label so the user knows something's happening during the ~10 s
// download. On success we re-load the panel so the banner re-renders
// against the now-installed prereq and the runtime section un-dims.
function wirePrereqBanner(panel, gameId) {
  const buttons = panel.querySelectorAll(".ce-prereq-install-btn");
  buttons.forEach(btn => {
    btn.addEventListener("click", async () => {
      const kind = btn.getAttribute("data-kind") || "";
      const original = btn.textContent;
      btn.disabled = true;
      btn.textContent = "Installing…";
      try {
        const result = await invoke("cheat_runtime_prereqs_install", {
          appId: String(gameId),
          kind,
        });
        const sizeMB = (result.size_bytes / (1024 * 1024)).toFixed(1);
        const tag = result.release_tag || "latest";
        btn.textContent = `✓ Installed ${prereqLabel(kind)} ${esc(tag)} (${sizeMB} MB)`;
        // Re-render so the banner flips to the success state + runtime
        // section un-dims. Brief delay so the user reads the success
        // text on the button before the panel re-renders.
        setTimeout(() => loadCheats(gameId), 800);
      } catch (e) {
        btn.disabled = false;
        btn.textContent = `✗ ${original} — ${String(e)}`;
        btn.title = String(e);
      }
    });
  });
}

function renderRuntimeSection(features, gameId, prereqsBlocked, ceTableName) {
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
  // Liveness pill: at least one feature reports the game running.
  // Walk the tree because the flag rides on every node, not just roots.
  const anyRunning = anyNodeRunning(features);
  const pillCls = anyRunning ? "cheat-pill-on" : "cheat-pill-off";
  const pillTxt = anyRunning ? "\u{1F7E2} Game running" : "\u{1F534} Game not running";

  // Global "Refresh all values" button — clicks every Value row's per-row
  // read button in sequence. Required by #133 acceptance; the per-row ↻
  // covers individual reads but a 50+ entry table (Dragon's Dogma 2) makes
  // that impractical for an "I just enabled the master cheat, refresh
  // everything" workflow.
  const blockedAttr = prereqsBlocked ? ' data-prereq-blocked="true"' : "";
  let html =
    `<div class="cheat-runtime-section"${blockedAttr}>` +
      `<div class="cheat-runtime-header">` +
        `Trainer features <span class="cheat-pill ${pillCls}">${pillTxt}</span>` +
        `<button class="cheat-refresh-all" data-action="refresh-all" title="Read every Value row's current value">Refresh all values</button>` +
      `</div>` +
      `<ul class="cheat-tree" data-game-id="${esc(String(gameId))}">`;
  for (const f of features) {
    html += renderTreeNode(f, gameId, 0, ceTableName);
  }
  html += `</ul></div>`;
  return html;
}

function anyNodeRunning(features) {
  for (const f of features) {
    if (f.game_running) return true;
    if (f.children && anyNodeRunning(f.children)) return true;
  }
  return false;
}

/// Render one node and its subtree. CE tables nest up to 7 levels in the
/// wild (Dragon's Dogma 2 v6, Elden Ring All-in-One); the renderer doesn't
/// cap depth — CSS handles indentation by natural <ul> nesting.
function renderTreeNode(f, gameId, depth, ceTableName) {
  const hasChildren = Array.isArray(f.children) && f.children.length > 0;
  const expanded = isExpanded(gameId, f.uuid);
  const expandedAttr = expanded ? "true" : "false";

  let rowHtml;
  if (f.kind === "header") {
    // Headers with children are interactive (caret + click expands).
    // Childless Headers stay as visual dividers — mirrors CE's
    // GroupHeader semantics (MemoryRecordUnit.pas:148).
    const caret = hasChildren
      ? `<span class="cheat-tree-caret">${expanded ? "▼" : "▶"}</span>`
      : `<span class="cheat-tree-caret cheat-tree-caret-empty"></span>`;
    rowHtml =
      `<div class="cheat-tree-row cheat-tree-header" data-expanded="${expandedAttr}"` +
        ` data-uuid="${esc(f.uuid)}" data-has-children="${hasChildren ? 'true' : 'false'}">` +
        caret +
        `<div class="cheat-tree-name">${esc(f.name)}</div>` +
      `</div>`;
  } else if (f.kind === "value") {
    rowHtml = renderValueRow(f);
  } else {
    rowHtml = renderToggleRow(f, ceTableName);
  }

  // Wrap each node in <li> so semantic structure is preserved
  // (assistive tech, keyboard tabbing). Children render as a nested
  // <ul> the caret toggles via the `data-expanded` attribute on the
  // parent row; CSS hides the <ul> when collapsed.
  let html = `<li class="cheat-tree-node" data-depth="${depth}">${rowHtml}`;
  if (hasChildren) {
    html += `<ul class="cheat-tree-children">`;
    for (const child of f.children) {
      html += renderTreeNode(child, gameId, depth + 1, ceTableName);
    }
    html += `</ul>`;
  }
  html += `</li>`;
  return html;
}

function renderToggleRow(f, ceTableName) {
  const cat = f.category ? `${esc(f.category)} • ` : "";
  const info =
    `<div class="cheat-runtime-info">` +
      `<div class="cheat-runtime-name">${esc(f.name)}</div>` +
      `<div class="cheat-runtime-meta">${cat}${esc(f.manifest_exe)}</div>` +
    `</div>`;

  // needs_ce: the script depends on a pointer root only CE's Lua framework
  // binds (e.g. CharacterManagerPtr). The native executor can't run it, so
  // we never show a toggle that would flip on and instantly fail. Surface a
  // "needs CE" badge + an Open-in-CE shortcut (reuses the .ce-open-btn
  // handler wired by wireTables) instead.
  if (f.needs_ce) {
    const open = ceTableName
      ? `<button class="ce-open-btn cheat-needs-ce-open" data-table="${esc(ceTableName)}"` +
          ` title="This cheat uses the table's Lua framework — run it in Cheat Engine">Open in CE</button>`
      : "";
    return (
      `<div class="cheat-tree-row cheat-runtime-item cheat-runtime-needs-ce">` +
        info +
        `<div class="cheat-needs-ce">` +
          `<span class="cheat-needs-ce-badge" title="Depends on a pointer root resolved by the table's Lua framework, which tatu can't run natively">🔒 Needs CE</span>` +
          open +
        `</div>` +
      `</div>`
    );
  }

  const dis = f.game_running ? "" : "disabled";
  const ch = f.active ? "checked" : "";
  return (
    `<div class="cheat-tree-row cheat-runtime-item">` +
      info +
      `<label class="cheat-switch" title="${dis ? 'Launch the game first' : 'Toggle'}">` +
        `<input type="checkbox" data-feature-uuid="${esc(f.uuid)}" ${ch} ${dis}>` +
        `<span class="cheat-switch-slider"></span>` +
      `</label>` +
    `</div>`
  );
}

// --- expand/collapse state (LocalStorage, scoped by gameId + uuid) ---
//
// Default: expanded on first encounter. Once the user collapses a node we
// remember it forever (across reloads, across sessions). Stored as a flat
// key-per-uuid rather than a JSON blob so we don't need to load + parse
// the whole tree state to read a single node, and so multiple game panels
// open simultaneously don't trample each other's writes.

function expandKey(gameId, uuid) {
  return `tatu.cheats.exp.${gameId}.${uuid}`;
}

function isExpanded(gameId, uuid) {
  try {
    const v = localStorage.getItem(expandKey(gameId, uuid));
    return v === null ? true : v === "1";
  } catch (_) {
    return true; // localStorage unavailable (private mode): default open.
  }
}

function setExpanded(gameId, uuid, expanded) {
  try {
    localStorage.setItem(expandKey(gameId, uuid), expanded ? "1" : "0");
  } catch (_) { /* ignore */ }
}

/// Render one Value-kind row: typed numeric input + Set + Freeze. Dimmed
/// (with reason) when the game isn't running OR the master scaffold cheat
/// hasn't been enabled — the backend flags both via `game_running` and
/// `symbol_ready` in the FeatureView. The wrapper `<div>` substitutes
/// for the old `<li>` because tree rows now compose into `cheat-tree-node`
/// `<li>` parents.
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
    `<div class="cheat-tree-row cheat-runtime-item cheat-runtime-value ${blocked ? 'cheat-runtime-value-blocked' : ''}" data-feature-uuid="${esc(f.uuid)}" data-vtype="${esc(vt)}">` +
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
    `</div>`
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

/// Wire the per-row "✗" remove button: confirm + invoke backend + refresh
/// panel. Lets the user prune minimalist / superseded / broken tables
/// without leaving the app — counterpart to the Import .CT button.
function wireRemoveTableButtons(panel, gameId) {
  panel.querySelectorAll(".ce-remove-btn").forEach(btn => {
    btn.addEventListener("click", async () => {
      const tableName = btn.dataset.table;
      // confirm() is intentionally simple here — the action is reversible
      // (the user can re-import the same .CT) but the in-app deletion is
      // immediate, so a single yes/no prompt strikes the right balance.
      if (!confirm(`Delete "${tableName}" and its converted manifest?`)) return;
      btn.disabled = true;
      const original = btn.textContent;
      btn.textContent = "…";
      try {
        await invoke("cheat_runtime_remove_ct", {
          appId: String(gameId),
          fileName: tableName,
        });
        loadCheats(gameId);
      } catch (e) {
        btn.textContent = "✗";
        btn.title = String(e);
        btn.disabled = false;
        setTimeout(() => btn.removeAttribute("title"), 3000);
      } finally {
        // loadCheats re-renders the panel so the button reference is gone
        // anyway, but reset textContent in case the call errored.
        btn.textContent = original;
      }
    });
  });
}

/// Wire the "Import .CT" button: trigger hidden <input type="file">, read
/// the picked file as bytes, hand off to the backend command which copies
/// into `cheat-tables/<app_id>/` and runs `auto_import_for_app` so the new
/// manifest appears in the list without a restart.
function wireImportButton(panel, gameId) {
  const btn = panel.querySelector(".ce-import-btn");
  const input = panel.querySelector(".ce-import-input");
  if (!btn || !input) return;

  btn.addEventListener("click", () => input.click());

  input.addEventListener("change", async () => {
    const file = input.files && input.files[0];
    if (!file) return;
    const original = btn.textContent;
    btn.disabled = true;
    btn.textContent = "Importing…";
    try {
      const buf = await file.arrayBuffer();
      const contents = Array.from(new Uint8Array(buf));
      const summary = await invoke("cheat_runtime_import_ct", {
        appId: String(gameId),
        fileName: file.name,
        contents,
      });
      // Summary fields (post-#134, JSON sidecars dropped): imported[],
      // failed[][file, err], written_to. The "skipped" bucket from the old
      // shape went away — the importer no longer round-trips through a JSON
      // cache, so there's nothing to deduplicate against.
      const imported = (summary.imported || []).length;
      const failed = (summary.failed || []).length;
      let msg;
      let isError = false;
      if (failed > 0) {
        const [name, err] = summary.failed[0];
        msg = `✗ ${name}: ${err}`;
        isError = true;
      } else if (imported > 0) {
        msg = `✓ Imported`;
      } else {
        msg = `✓ Done`;
      }
      btn.textContent = msg;
      btn.title = msg; // full text in tooltip in case the button truncates
      loadCheats(gameId);
      if (isError) {
        // Keep the failure message visible until the user clicks the button
        // again — 3 seconds disappears too fast to read a NoExeBinding-class
        // error and decide what to do next.
        btn.disabled = false;
        const dismiss = () => {
          btn.textContent = original;
          btn.removeAttribute("title");
          btn.removeEventListener("click", dismiss, true);
        };
        btn.addEventListener("click", dismiss, true);
      } else {
        setTimeout(() => {
          btn.textContent = original;
          btn.removeAttribute("title");
          btn.disabled = false;
        }, 3000);
      }
    } catch (e) {
      btn.textContent = "✗ Error";
      btn.title = String(e);
      setTimeout(() => {
        btn.textContent = original;
        btn.removeAttribute("title");
        btn.disabled = false;
      }, 3000);
    } finally {
      // Reset so the same file can be re-picked (no change event on
      // identical selection otherwise).
      input.value = "";
    }
  });
}

/// Wire the global "Refresh all values" button — clicks every Value row's
/// per-row ↻ in sequence. Sequenced, not parallel: parallel reads against
/// the same ptrace target serialize at the kernel anyway and would race
/// the master toggle's symbol table; one-at-a-time is both safer and the
/// user-perceived performance hit is negligible.
function wireRefreshAll(panel) {
  const btn = panel.querySelector(".cheat-refresh-all");
  if (!btn) return;
  btn.addEventListener("click", async () => {
    const reads = panel.querySelectorAll(".cheat-runtime-value:not(.cheat-runtime-value-blocked) .cheat-value-read");
    if (!reads.length) return;
    btn.disabled = true;
    const original = btn.textContent;
    btn.textContent = `Reading 0 / ${reads.length}…`;
    let done = 0;
    for (const r of reads) {
      // Reuse the per-row handler's behaviour — a synthetic click fires
      // the same async invoke + input update + flash sequence already
      // wired in wireValueRows, so refresh-all stays a single source of
      // truth for what a "read" actually does.
      r.click();
      done += 1;
      btn.textContent = `Reading ${done} / ${reads.length}…`;
      // Small yield so the UI repaints; per-row invokes themselves are
      // already async and don't block here.
      await new Promise(r => setTimeout(r, 16));
    }
    btn.textContent = original;
    btn.disabled = false;
  });
}

/// Wire expand/collapse on every Header row that owns children. Click
/// toggles the row's `data-expanded` attribute, which CSS keys off to hide
/// the sibling `.cheat-tree-children` <ul>. State persists in
/// LocalStorage scoped by gameId so a closed group stays closed across
/// reloads (matters for the 100+ entry tables in the FearLess audit).
function wireTreeCarets(panel, gameId) {
  panel.querySelectorAll('.cheat-tree-header[data-has-children="true"]').forEach(row => {
    row.addEventListener("click", () => {
      const expanded = row.dataset.expanded === "true";
      const next = !expanded;
      row.dataset.expanded = next ? "true" : "false";
      const caret = row.querySelector(".cheat-tree-caret");
      if (caret) caret.textContent = next ? "▼" : "▶";
      setExpanded(gameId, row.dataset.uuid, next);
    });
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
        if (desired) {
          // Enable failed. Most static "needs CE" cases are caught up front
          // (the row renders as a badge, not a toggle), but a cheat can still
          // fail at runtime — unsupported asm, an AOB that didn't match this
          // build, a symbol bound only when a sibling is active. Surface a
          // persistent reason instead of a 3s flash so the user understands
          // the toggle didn't just bounce.
          showRuntimeError(input, String(e));
        } else {
          input.title = String(e);
          setTimeout(() => input.removeAttribute("title"), 3000);
        }
      } finally {
        input.disabled = false;
      }
    });
  });
}

// Pin a persistent error reason under a toggle row whose enable failed.
// Replaces the old 3s-tooltip flash. Cleared on the next loadCheats()
// re-render (i.e. the next time the user interacts or refreshes).
function showRuntimeError(input, raw) {
  const row = input.closest(".cheat-runtime-item");
  if (!row) {
    input.title = raw;
    setTimeout(() => input.removeAttribute("title"), 3000);
    return;
  }
  row.classList.add("cheat-runtime-failed");
  let msg = row.querySelector(".cheat-runtime-error-msg");
  if (!msg) {
    msg = document.createElement("div");
    msg.className = "cheat-runtime-error-msg";
    row.appendChild(msg);
  }
  msg.textContent = `⚠ ${friendlyEnableError(raw)}`;
  msg.title = raw; // full error on hover
}

// Translate the executor's raw error into a short, actionable line. The
// common runtime failures for framework tables all point the user at CE.
function friendlyEnableError(raw) {
  const e = raw.toLowerCase();
  if (e.includes("no match")) {
    return "pattern not found in memory — table may not match this game build";
  }
  if (e.includes("unknown symbol") || e.includes("outside any label")) {
    return "unresolved symbol — likely needs CE (open the table in Cheat Engine)";
  }
  if (e.includes("unsupported") || e.includes("estimate length")) {
    return "uses an instruction tatu can't assemble yet — open the table in CE";
  }
  if (e.includes("lua")) {
    return "Lua-only script — open the table in Cheat Engine";
  }
  return raw.length > 120 ? `${raw.slice(0, 117)}…` : raw;
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
    const [ceStatus, tables, runtimeFeatures, orphans, prereqs] = await Promise.all([
      invoke("ce_install_status").catch(() => ({ kind: "not_installed" })),
      invoke("ce_list_tables_for_game", { appId: String(gameId) }).catch(() => []),
      invoke("cheat_runtime_list_features", { appId: String(gameId) }).catch(() => []),
      invoke("cheat_runtime_orphans_list").catch(() => []),
      // #98 prereqs check. Surface failure as empty list (no banner)
      // rather than blocking the whole panel — same defensive pattern
      // as the other four calls above.
      invoke("cheat_runtime_prereqs_check", { appId: String(gameId) }).catch(() => []),
    ]);

    if (state.panelGameId !== gameId) return;

    const banner = renderCeBanner(ceStatus);
    const prereqBanner = renderPrereqBanner(prereqs);
    const blocked = anyPrereqBlocking(prereqs);
    // Pass the first .CT table name so "Needs CE" rows can offer a direct
    // Open-in-CE shortcut. Most games have a single table; when several
    // exist we point at the first (the user can still use the full list
    // in the tables section below).
    const ceTableName = Array.isArray(tables) && tables.length ? tables[0].name : null;
    const runtimeSection = renderRuntimeSection(runtimeFeatures, gameId, blocked, ceTableName);
    const tablesSection = renderTablesSection(gameId, tables);
    const searchBar = renderSearchBar(gameId);

    const orphansBanner = renderOrphansBanner(orphans, gameId);
    panel.innerHTML = banner + orphansBanner + prereqBanner + runtimeSection + tablesSection + searchBar;

    wireBanner(panel, gameId);
    wireOrphansBanner(panel, gameId);
    wirePrereqBanner(panel, gameId);
    wireTreeCarets(panel, gameId);
    wireRuntimeSwitches(panel, gameId);
    wireValueRows(panel, gameId);
    wireRefreshAll(panel);
    wireTables(panel, gameId);
    wireSearchButton(panel);
  } catch (e) {
    if (state.panelGameId !== gameId) return;
    panel.innerHTML = `<div class="ach-empty">Error loading cheats: ${esc(String(e))}</div>`;
  }
}
