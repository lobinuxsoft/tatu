import { invoke } from "../tauri.js";
import { esc } from "../utils.js";
import { loadingPlaceholder } from "../panel/detail_template.js";
import { CONFIG_FIELDS, configFormHtml, configValueText, configValuesHtml, readConfigForm } from "../panel/gamenative_config.js";
import { CHARTS, FPS_NOTE, SUMMARY_ROWS, fmt, fmtDate, fmtTime, insightsHtml, samplesTableHtml, summaryTilesHtml, summaryValue } from "./metrics.js";
import { renderChart, slotColor } from "./chart.js";

// "Sesiones": every saved recording of this game, and comparing 2–4 of them.
// The oldest one ticked is the base: every other shows what changed against
// it and what that did (FPS, 1% low, power, temperatures), and the base's own
// values stay visible — reverting a change is copying them back.

const MAX_COMPARE = 4;

export function mountSessions(el, game) {
  // `slots`: a session keeps its color for as long as it stays selected —
  // unticking another one never repaints it.
  const ctx = { el, game, list: [], slots: new Map(), cache: new Map(), view: null, redraw: () => {} };
  return { refresh: () => refresh(ctx), redraw: () => ctx.redraw() };
}

async function refresh(ctx) {
  ctx.el.innerHTML = loadingPlaceholder("Cargando sesiones...");
  try {
    ctx.list = await invoke("perf_list", { source: ctx.game.source, id: ctx.game.id });
  } catch (e) {
    ctx.el.innerHTML = `<div class="cartridge-warn">${esc(String(e))}</div>`;
    return;
  }
  const ids = new Set(ctx.list.map(s => s.id));
  for (const id of [...ctx.slots.keys()]) if (!ids.has(id)) ctx.slots.delete(id);
  // Back to the list (and the comparison, if 2+ are ticked) on every refresh.
  ctx.view = null;
  render(ctx);
}

function render(ctx) {
  if (!ctx.list.length) {
    ctx.el.innerHTML = `<div class="modal-hint">Todavía no hay sesiones guardadas para este juego. Medí una desde "En vivo".</div>`;
    return;
  }
  const rows = ctx.list
    .map(s => {
      const slot = ctx.slots.get(s.id);
      const key = slot !== undefined ? `<i class="perf-key" style="background:${slotColor(slot)}"></i>` : "";
      return (
        `<tr data-id="${s.id}">` +
        `<td><input type="checkbox" data-s="pick"${slot !== undefined ? " checked" : ""}> ${key}</td>` +
        `<td>${esc(fmtDate(s.id))}</td>` +
        `<td>${esc(s.label || "Sin nombre")}</td>` +
        `<td>${esc((s.config && s.config.name) || "—")}</td>` +
        `<td>${esc(s.tdp_note || "—")}</td>` +
        `<td>${esc(fmtTime(s.summary.duration_s))}</td>` +
        `<td>${fmt(s.summary.avg_fps, 1)}</td>` +
        `<td>${fmt(s.summary.low1_fps, 1)}</td>` +
        `<td>${fmt(s.summary.avg_power_w, 2)}</td>` +
        `<td class="perf-actions"><button class="cartridge-btn-secondary" data-s="view">Ver</button>` +
        `<button class="cartridge-btn-secondary" data-s="edit">Editar</button></td>` +
        `</tr>`
      );
    })
    .join("");
  ctx.el.innerHTML =
    `<div class="modal-hint">Marcá de 2 a ${MAX_COMPARE} sesiones para compararlas, o tocá "Ver" para abrir una.</div>` +
    `<div class="perf-table-wrap"><table class="perf-table perf-sessions">` +
    `<tr><th></th><th>Fecha</th><th>Sesión</th><th>Configuración</th><th>Modo / TDP</th><th>Duración</th>` +
    `<th>FPS prom.</th><th>1% low</th><th>Consumo (W)</th><th></th></tr>${rows}</table></div>` +
    `<div class="perf-view" data-s="view-area"></div>`;

  ctx.el.querySelector(".perf-sessions").onclick = e => {
    const tr = e.target.closest("tr[data-id]");
    const action = e.target.dataset && e.target.dataset.s;
    if (!tr || !action) return;
    const header = ctx.list.find(s => s.id === Number(tr.dataset.id));
    if (action === "pick") togglePick(ctx, header.id, e.target);
    else if (action === "view") showSession(ctx, header);
    else if (action === "edit") editSession(ctx, header);
  };
  renderView(ctx);
}

function togglePick(ctx, id, box) {
  if (ctx.slots.has(id)) {
    ctx.slots.delete(id);
  } else {
    if (ctx.slots.size >= MAX_COMPARE) {
      box.checked = false;
      return;
    }
    const used = new Set(ctx.slots.values());
    ctx.slots.set(id, [0, 1, 2, 3].find(s => !used.has(s)));
  }
  ctx.view = null;
  render(ctx);
}

function viewArea(ctx) {
  return ctx.el.querySelector('[data-s="view-area"]');
}

function renderView(ctx) {
  if (ctx.view) return; // a single session is open; it re-renders itself
  if (ctx.slots.size >= 2) compare(ctx);
  else ctx.redraw = () => {};
}

async function loadSession(ctx, header) {
  if (!ctx.cache.has(header.id)) {
    ctx.cache.set(header.id, await invoke("perf_load", { source: ctx.game.source, id: ctx.game.id, session: header.id }));
  }
  return ctx.cache.get(header.id);
}

async function showSession(ctx, header) {
  ctx.view = header;
  const area = viewArea(ctx);
  area.innerHTML = loadingPlaceholder("Cargando la sesión...");
  let session;
  try {
    session = await loadSession(ctx, header);
  } catch (e) {
    area.innerHTML = `<div class="cartridge-warn">${esc(String(e))}</div>`;
    return;
  }
  const h = session;
  area.innerHTML =
    `<div class="perf-view-head"><b>${esc(h.label || "Sin nombre")}</b> · ${esc(fmtDate(h.id))} · ${esc(h.device.model || h.device.serial)}` +
    (h.tdp_note ? ` · ${esc(h.tdp_note)}` : "") +
    `</div>` +
    (h.error ? `<div class="cartridge-warn">Terminó antes de tiempo: ${esc(h.error)}</div>` : "") +
    `<div class="perf-tiles">${summaryTilesHtml(h.summary)}</div>` +
    `<div data-s="insights"></div>` +
    (h.config ? `<div class="perf-config">${configValuesHtml(h.config)}</div>` : `<div class="modal-hint">Sin configuración de GameNative anotada.</div>`) +
    `<div class="cartridge-actions">` +
    (h.config ? `<button class="cartridge-btn-secondary" data-s="best">Usar esta configuración como la mejor</button>` : "") +
    `<button class="cartridge-btn-secondary" data-s="delete">Borrar sesión</button>` +
    `<button class="cartridge-btn-secondary" data-s="close">Cerrar</button></div>` +
    `<div data-s="msg"></div>` +
    `<div class="perf-charts">${CHARTS.map((_, i) => `<div class="perf-chart" data-chart="${i}"></div>`).join("")}</div>` +
    `<p class="modal-hint">${esc(FPS_NOTE)}</p>` +
    `<details class="perf-details"><summary>Ver datos (tabla)</summary>${samplesTableHtml(h.samples)}</details>`;

  const msg = html => (area.querySelector('[data-s="msg"]').innerHTML = html);
  const best = area.querySelector('[data-s="best"]');
  if (best) {
    best.onclick = async () => {
      try {
        await invoke("gamenative_save_config", { source: ctx.game.source, id: ctx.game.id, config: h.config });
        msg(`<div class="import-result import-result-ok">Guardada como la mejor configuración del juego (pestaña GameNative).</div>`);
      } catch (e) {
        msg(`<div class="cartridge-warn">${esc(String(e))}</div>`);
      }
    };
  }
  // Two clicks, no dialog: deleting a recording can't be undone.
  const del = area.querySelector('[data-s="delete"]');
  del.onclick = async () => {
    if (!del.dataset.armed) {
      del.dataset.armed = "1";
      del.textContent = "¿Seguro? Tocá de nuevo para borrar";
      return;
    }
    try {
      await invoke("perf_delete", { source: ctx.game.source, id: ctx.game.id, session: h.id });
      ctx.cache.delete(h.id);
      ctx.slots.delete(h.id);
      ctx.view = null;
      refresh(ctx);
    } catch (e) {
      msg(`<div class="cartridge-warn">${esc(String(e))}</div>`);
    }
  };
  area.querySelector('[data-s="close"]').onclick = () => {
    ctx.view = null;
    render(ctx);
  };
  ctx.redraw = () => area.querySelectorAll("[data-chart]").forEach(el => renderChart(el, CHARTS[Number(el.dataset.chart)], h.samples));
  ctx.redraw();
  fillInsights(area.querySelector('[data-s="insights"]'), h.game, h.id);
}

// Computed by the backend on every view, from the session's samples and the
// config it ran with. Shared with "En vivo".
export async function fillInsights(el, game, session) {
  try {
    el.innerHTML = insightsHtml(await invoke("perf_insights", { source: game.source, id: game.id, session }));
  } catch (e) {
    el.innerHTML = `<div class="cartridge-warn">${esc(String(e))}</div>`;
  }
}

function editSession(ctx, header) {
  ctx.view = header;
  ctx.redraw = () => {};
  sessionEditForm(viewArea(ctx), header, {
    onSaved: updated => {
      const i = ctx.list.findIndex(s => s.id === updated.id);
      if (i >= 0) ctx.list[i] = updated;
      ctx.cache.delete(updated.id);
      ctx.view = null;
      render(ctx);
    },
    onCancel: () => {
      ctx.view = null;
      render(ctx);
    },
  });
}

// The session's own name, the TDP / performance mode it ran at (Android
// exposes none to read) and the GameNative config it used — prefilled from
// the game's saved best config when recording started. Shared with "En vivo".
export function sessionEditForm(el, header, { onSaved, onCancel } = {}) {
  el.innerHTML =
    `<label class="gn-field gn-field-wide">Nombre de la sesión` +
    `<input type="text" class="gn-input" data-s="label" value="${esc(header.label || "")}" placeholder="ej. Turnip + DXVK async"></label>` +
    `<label class="gn-field gn-field-wide">Modo de rendimiento / TDP del dispositivo` +
    `<input type="text" class="gn-input" data-s="tdp" value="${esc(header.tdp_note || "")}" placeholder="ej. Alto rendimiento, 15 W, ventilador en Auto"></label>` +
    `<div class="detail-info-row"><span class="detail-info-label">Configuración de GameNative usada</span></div>` +
    configFormHtml(header.config) +
    `<div class="cartridge-actions"><button class="cartridge-btn" data-s="save">Guardar</button>` +
    (onCancel ? `<button class="cartridge-btn-secondary" data-s="cancel">Cancelar</button>` : "") +
    `</div><div data-s="msg"></div>`;
  el.querySelector('[data-s="save"]').onclick = async () => {
    try {
      const updated = await invoke("perf_update", {
        source: header.game.source,
        id: header.game.id,
        session: header.id,
        label: el.querySelector('[data-s="label"]').value,
        tdpNote: el.querySelector('[data-s="tdp"]').value,
        config: readConfigForm(el),
      });
      el.querySelector('[data-s="msg"]').innerHTML = `<div class="import-result import-result-ok">Guardado.</div>`;
      if (onSaved) onSaved(updated);
    } catch (e) {
      el.querySelector('[data-s="msg"]').innerHTML = `<div class="cartridge-warn">${esc(String(e))}</div>`;
    }
  };
  if (onCancel) el.querySelector('[data-s="cancel"]').onclick = onCancel;
}

// FPS rows compare in %, everything else in its own unit.
const PERCENT_ROWS = new Set(["avg_fps", "low1_fps", "min_fps", "max_fps"]);

function delta(row, base, value) {
  if (row.time || row.key === "frames" || base == null || value == null) return "";
  if (PERCENT_ROWS.has(row.key)) {
    if (!base) return "";
    const p = ((value - base) / base) * 100;
    return ` (${p >= 0 ? "+" : ""}${p.toFixed(0)}%)`;
  }
  const d = value - base;
  return ` (${d >= 0 ? "+" : ""}${d.toFixed(row.digits)}${row.unit || ""})`;
}

// The headline of a comparison: per session, the settings it changed against
// the base, and what happened to the numbers that matter most.
const EFFECT_ROWS = ["avg_fps", "low1_fps", "pacing_pct", "avg_power_w", "max_temp_cpu", "max_temp_battery"];

function effectsHtml(sessions, settings, color) {
  const base = sessions[0];
  return sessions
    .slice(1)
    .map(s => {
      const changes = settings
        .map(f => [f.label, f.get(base), f.get(s)])
        .filter(([, a, b]) => a !== b)
        .map(([label, a, b]) => `${label}: ${a} → ${b}`);
      const effects = EFFECT_ROWS.map(key => SUMMARY_ROWS.find(r => r.key === key))
        .map(row => [row, delta(row, base.summary[row.key], s.summary[row.key])])
        .filter(([, d]) => d)
        .map(([row, d]) => `${row.label}${d}`);
      return (
        `<div class="perf-effect"><i class="perf-key" style="background:${color(s)}"></i>` +
        `<b>${esc(s.label || fmtDate(s.id))}</b>` +
        `<div>Cambios: ${esc(changes.length ? changes.join(" · ") : "ninguno anotado")}</div>` +
        `<div>Efecto: ${esc(effects.length ? effects.join(" · ") : "sin datos comparables")}</div>` +
        `</div>`
      );
    })
    .join("");
}

async function compare(ctx) {
  const area = viewArea(ctx);
  // Oldest first: the first recording is the base the others changed from.
  const picked = ctx.list.filter(s => ctx.slots.has(s.id)).sort((a, b) => a.id - b.id);
  area.innerHTML = loadingPlaceholder("Cargando la comparación...");
  let sessions;
  try {
    sessions = await Promise.all(picked.map(h => loadSession(ctx, h)));
  } catch (e) {
    area.innerHTML = `<div class="cartridge-warn">${esc(String(e))}</div>`;
    return;
  }
  const color = s => slotColor(ctx.slots.get(s.id));
  const head =
    `<tr><th></th>` +
    sessions
      .map((s, i) => `<th><i class="perf-key" style="background:${color(s)}"></i>${esc(s.label || fmtDate(s.id))}${i === 0 ? " (base)" : ""}</th>`)
      .join("") +
    `</tr>`;

  // Best value marked with ★ and weight, never color alone; every other
  // session also shows its change against the base.
  const metricRows = SUMMARY_ROWS.map(row => {
    const values = sessions.map(s => s.summary[row.key]);
    const known = values.filter(v => v !== null && v !== undefined);
    const best = row.better && known.length > 1 ? (row.better === "high" ? Math.max(...known) : Math.min(...known)) : null;
    const cells = sessions
      .map((s, i) => {
        const isBest = best !== null && values[i] === best;
        const change = i === 0 ? "" : delta(row, values[0], values[i]);
        return `<td${isBest ? ' class="perf-best"' : ""}>${esc(summaryValue(row, s.summary) + change)}${isBest ? " ★" : ""}</td>`;
      })
      .join("");
    return `<tr><td>${esc(row.label)}</td>${cells}</tr>`;
  }).join("");

  // What changed: only the settings that differ between these sessions.
  const settings = [
    { label: "Configuración", get: s => (s.config && s.config.name) || "—" },
    { label: "Modo / TDP", get: s => s.tdp_note || "—" },
    { label: "Dispositivo", get: s => s.device.model || s.device.serial },
    ...CONFIG_FIELDS.map(f => ({ label: f.label, get: s => configValueText(f, s.config ? s.config.container[f.key] : undefined) })),
    { label: "Notas", get: s => (s.config && s.config.notes) || "—" },
  ];
  const diffRows = settings
    .map(f => ({ f, values: sessions.map(f.get) }))
    .filter(({ values }) => new Set(values).size > 1)
    .map(({ f, values }) => `<tr><td>${esc(f.label)}</td>${values.map(v => `<td class="perf-diff">${esc(v)}</td>`).join("")}</tr>`)
    .join("");

  area.innerHTML =
    `<div class="detail-info-row"><span class="detail-info-label">Cambio → efecto</span></div>` +
    `<div class="perf-effects">${effectsHtml(sessions, settings, color)}</div>` +
    `<div class="perf-chart" data-s="fps"></div>` +
    `<div class="perf-table-wrap"><table class="perf-table">${head}${metricRows}</table></div>` +
    `<div class="detail-info-row"><span class="detail-info-label">Qué cambió</span></div>` +
    (diffRows
      ? `<div class="perf-table-wrap"><table class="perf-table perf-diffs">${head}${diffRows}</table></div>`
      : `<div class="modal-hint">Misma configuración anotada en todas — anotá qué cambiaste en cada sesión (botón Editar).</div>`) +
    `<p class="modal-hint">Para volver atrás un cambio, copiá en GameNative el valor de la columna "(base)".</p>` +
    `<p class="modal-hint">${esc(FPS_NOTE)}</p>`;

  // FPS over time, one line per session, aligned from each recording's start.
  const length = Math.max(...sessions.map(s => s.samples.length));
  const rows = Array.from({ length }, (_, i) => {
    const row = { t: i };
    sessions.forEach((s, j) => (row[`s${j}`] = s.samples[i] ? s.samples[i].fps : null));
    return row;
  });
  const chart = {
    title: "FPS por sesión",
    unit: "fps",
    series: sessions.map((s, j) => ({ key: `s${j}`, name: s.label || fmtDate(s.id) })),
  };
  ctx.redraw = () => renderChart(area.querySelector('[data-s="fps"]'), chart, rows, { colors: sessions.map(color) });
  ctx.redraw();
}
