import { invoke, listen } from "../tauri.js";
import { esc } from "../utils.js";
import { showDevicePicker } from "../panel/android_devices.js";
import { loadingPlaceholder } from "../panel/detail_template.js";
import { CHARTS, FPS_NOTE, fmtTime, liveTilesHtml, samplesTableHtml, statusHtml, summaryTilesHtml } from "./metrics.js";
import { renderChart } from "./chart.js";
import { fillInsights, sessionEditForm } from "./sessions.js";

// "En vivo": device → start → a sample per second (tiles, charts, what the
// probe can and can't see) → stop. The backend saves the session the moment
// recording ends, so what's left here is naming it: the label, the TDP /
// performance mode, and the GameNative config it ran with.

let unlisteners = [];
let redraw = () => {};

const WIFI_NOTE =
  `<div class="cartridge-guide">Abrí el juego en GameNative y elegí el dispositivo. Para medir el <b>consumo</b> ` +
  `conectalo por <b>Wi-Fi</b> y desenchufado: por cable el teléfono se está cargando y el consumo real no se puede leer.</div>`;

function stopListening() {
  unlisteners.forEach(u => u());
  unlisteners = [];
}

// `onSaved`: the session list refreshes after a session is named.
export function mountLive(el, game, { onSaved }) {
  stopListening();
  const ctx = { el, game, onSaved, device: null, samples: [], live: null };
  pick(ctx);
  return { redraw: () => redraw() };
}

function q(ctx, selector) {
  return ctx.el.querySelector(selector);
}

function pick(ctx) {
  redraw = () => {};
  showDevicePicker(ctx.el, device => ready(ctx, device), { note: WIFI_NOTE });
}

function ready(ctx, device) {
  ctx.device = device;
  const via = device.serial.includes(":") ? "Wi-Fi" : "USB — el consumo no se va a poder medir";
  ctx.el.innerHTML =
    `<div class="cartridge-guide">Dispositivo: <b>${esc(device.model || device.serial)}</b> (${esc(via)})</div>` +
    `<div class="cartridge-actions">` +
    `<button class="cartridge-btn" data-p="start">▶ Iniciar medición</button>` +
    `<button class="cartridge-btn-secondary" data-p="change">Cambiar dispositivo</button>` +
    `</div>` +
    `<details class="perf-details" data-p="diag"><summary>Diagnóstico: qué puede leer este dispositivo</summary>` +
    `<div data-p="diagBody"></div></details>`;
  q(ctx, '[data-p="start"]').onclick = () => start(ctx);
  q(ctx, '[data-p="change"]').onclick = () => pick(ctx);

  // Nobody can test every phone: the raw probe output is what a report of
  // "X shows empty on my device" needs.
  const diag = q(ctx, '[data-p="diag"]');
  diag.addEventListener("toggle", async () => {
    const body = q(ctx, '[data-p="diagBody"]');
    if (!diag.open || body.dataset.loaded) return;
    body.dataset.loaded = "1";
    body.innerHTML = loadingPlaceholder("Consultando el dispositivo...");
    try {
      const raw = await invoke("perf_probe", { serial: device.serial });
      body.innerHTML = `<pre class="perf-raw">${esc(raw)}</pre>`;
    } catch (e) {
      body.innerHTML = `<div class="cartridge-warn">${esc(String(e))}</div>`;
    }
  });
}

async function start(ctx) {
  ctx.samples = [];
  ctx.live = null;
  ctx.el.innerHTML =
    `<div class="perf-toolbar">` +
    `<button class="cartridge-btn" data-p="stop">■ Detener</button>` +
    `<span class="perf-rec">● Midiendo</span><span class="perf-clock" data-p="clock">0:00</span>` +
    `</div>` +
    `<div class="perf-status" data-p="status"></div>` +
    `<div class="perf-tiles" data-p="tiles"></div>` +
    `<div class="perf-charts">${CHARTS.map((_, i) => `<div class="perf-chart" data-chart="${i}"></div>`).join("")}</div>` +
    `<p class="modal-hint">${esc(FPS_NOTE)}</p>`;
  q(ctx, '[data-p="stop"]').onclick = e => {
    e.target.disabled = true;
    e.target.textContent = "Deteniendo...";
    invoke("perf_stop").catch(() => {});
  };

  // Listeners in place before starting — samples begin right away.
  stopListening();
  unlisteners.push(
    await listen("perf_sample", e => {
      ctx.samples.push(e.payload.sample);
      ctx.live = e.payload.live;
      drawLive(ctx);
    }),
  );
  unlisteners.push(await listen("perf_done", e => done(ctx, e.payload || {})));

  try {
    await invoke("perf_start", { serial: ctx.device.serial, model: ctx.device.model || ctx.device.serial });
  } catch (e) {
    stopListening();
    showError(ctx, String(e));
    return;
  }
  redraw = () => drawCharts(ctx.el, ctx.samples);
  drawCharts(ctx.el, ctx.samples);
}

function drawLive(ctx) {
  const last = ctx.samples[ctx.samples.length - 1];
  const clock = q(ctx, '[data-p="clock"]');
  if (!clock) return;
  clock.textContent = fmtTime(last.t);
  q(ctx, '[data-p="status"]').innerHTML = statusHtml(last, ctx.device);
  q(ctx, '[data-p="tiles"]').innerHTML = liveTilesHtml(last, ctx.live);
  drawCharts(ctx.el, ctx.samples);
}

export function drawCharts(root, samples) {
  root.querySelectorAll("[data-chart]").forEach(el => renderChart(el, CHARTS[Number(el.dataset.chart)], samples));
}

function done(ctx, payload) {
  stopListening();
  if (!payload.session) {
    showError(ctx, payload.error || "La medición terminó sin datos");
    return;
  }
  const header = payload.session;
  ctx.el.innerHTML =
    `<div class="import-result import-result-ok">✓ Sesión guardada (${esc(fmtTime(header.summary.duration_s))}). Ponele un nombre y anotá qué cambiaste en la configuración (arranca con la de la sesión anterior).</div>` +
    (header.error ? `<div class="cartridge-warn">La medición terminó antes de tiempo: ${esc(header.error)}</div>` : "") +
    `<div class="perf-tiles">${summaryTilesHtml(header.summary)}</div>` +
    `<div data-p="insights"></div>` +
    `<div class="perf-edit" data-p="form"></div>` +
    `<div class="perf-charts">${CHARTS.map((_, i) => `<div class="perf-chart" data-chart="${i}"></div>`).join("")}</div>` +
    `<details class="perf-details"><summary>Ver datos (tabla)</summary>${samplesTableHtml(ctx.samples)}</details>` +
    `<div class="cartridge-actions"><button class="cartridge-btn-secondary" data-p="again">Nueva medición</button></div>`;
  // Editing the config it ran with can change the ideas: refresh them on save.
  const insights = q(ctx, '[data-p="insights"]');
  fillInsights(insights, header.game, header.id);
  sessionEditForm(q(ctx, '[data-p="form"]'), header, {
    onSaved: updated => {
      fillInsights(insights, updated.game, updated.id);
      if (ctx.onSaved) ctx.onSaved(updated);
    },
  });
  q(ctx, '[data-p="again"]').onclick = () => ready(ctx, ctx.device);
  redraw = () => drawCharts(ctx.el, ctx.samples);
  drawCharts(ctx.el, ctx.samples);
}

function showError(ctx, message) {
  redraw = () => {};
  ctx.el.innerHTML =
    `<div class="cartridge-warn">${esc(message)}</div>` +
    `<div class="cartridge-actions"><button class="cartridge-btn-secondary" data-p="back">Volver</button></div>`;
  q(ctx, '[data-p="back"]').onclick = () => (ctx.device ? ready(ctx, ctx.device) : pick(ctx));
}
