import { esc } from "../utils.js";

// What the performance window plots, summarizes and compares — one place, so
// the live view, a saved session and the comparison all read the same
// definitions. One unit per chart (never two y-axes); within a chart each
// series keeps its slot (CPU is always slot 1), so its color never changes.

export const CHARTS = [
  { title: "FPS", unit: "fps", series: [{ key: "fps", name: "FPS" }] },
  {
    title: "Tiempo por cuadro", unit: "ms",
    series: [{ key: "frametime_max_ms", name: "Peor cuadro del segundo" }, { key: "frametime_sd_ms", name: "Desvío (ritmo)" }],
  },
  {
    title: "Uso de CPU y GPU", unit: "%", max: 100,
    series: [{ key: "cpu_pct", name: "CPU (total)" }, { key: "cpu_core_max_pct", name: "Núcleo más cargado" }, { key: "gpu_pct", name: "GPU" }],
  },
  { title: "Frecuencias", unit: "MHz", series: [{ key: "cpu_mhz", name: "CPU (clúster más rápido)" }, { key: "gpu_mhz", name: "GPU" }] },
  { title: "Consumo del dispositivo", unit: "W", series: [{ key: "power_w", name: "Consumo" }] },
  {
    title: "Temperaturas", unit: "°C",
    series: [{ key: "temp_cpu", name: "CPU" }, { key: "temp_gpu", name: "GPU" }, { key: "temp_battery", name: "Batería" }, { key: "temp_skin", name: "Carcasa" }],
  },
  { title: "RAM usada (todo el dispositivo)", unit: "MB", series: [{ key: "ram_used_mb", name: "RAM" }] },
];

// Comparison rows. `better` only where the direction is unambiguous — more
// CPU/GPU use isn't better or worse on its own.
export const SUMMARY_ROWS = [
  { key: "avg_fps", label: "FPS promedio", digits: 1, better: "high" },
  { key: "low1_fps", label: "1% low", digits: 1, better: "high" },
  { key: "pacing_pct", label: "Ritmo (cuadros parejos)", unit: "%", digits: 0, better: "high" },
  { key: "frametime_sd_ms", label: "Desvío del tiempo por cuadro", unit: " ms", digits: 1, better: "low" },
  { key: "median_frametime_ms", label: "Tiempo típico por cuadro", unit: " ms", digits: 1 },
  { key: "min_fps", label: "Peor segundo (FPS)", digits: 0, better: "high" },
  { key: "max_fps", label: "Mejor segundo (FPS)", digits: 0 },
  { key: "avg_cpu_pct", label: "CPU promedio", unit: "%", digits: 0 },
  { key: "avg_cpu_core_max_pct", label: "Núcleo más cargado (prom.)", unit: "%", digits: 0 },
  { key: "avg_cpu_mhz", label: "Frecuencia CPU (prom.)", unit: " MHz", digits: 0 },
  { key: "cpu_cap_mhz", label: "Tope de CPU", unit: " MHz", digits: 0 },
  { key: "avg_gpu_pct", label: "GPU promedio", unit: "%", digits: 0 },
  { key: "avg_gpu_mhz", label: "Frecuencia GPU (prom.)", unit: " MHz", digits: 0 },
  { key: "gpu_cap_mhz", label: "Tope de GPU", unit: " MHz", digits: 0 },
  { key: "avg_power_w", label: "Consumo promedio", unit: " W", digits: 2, better: "low" },
  { key: "max_temp_cpu", label: "Temp. máx. CPU", unit: " °C", digits: 1, better: "low" },
  { key: "max_temp_gpu", label: "Temp. máx. GPU", unit: " °C", digits: 1, better: "low" },
  { key: "max_temp_battery", label: "Temp. máx. batería", unit: " °C", digits: 1, better: "low" },
  { key: "max_temp_skin", label: "Temp. máx. carcasa", unit: " °C", digits: 1, better: "low" },
  { key: "max_ram_used_mb", label: "RAM usada (pico)", unit: " MB", digits: 0 },
  { key: "duration_s", label: "Duración", time: true },
  { key: "frames", label: "Frames medidos", digits: 0 },
];

export const FPS_NOTE =
  "FPS = cuadros que presenta la superficie del juego en GameNative (lo mismo que cuenta su propio contador). " +
  "1% low = FPS promedio del 1% de cuadros más lentos (se calcula desde 100 cuadros). " +
  "Ritmo = % de cuadros que llegaron a la pantalla a ±10% del tiempo típico (ej. 33,3 ms a 30 fps): 100% es un ritmo de consola; " +
  "un \"30 fps\" que alterna 16,7 y 50 ms marca ~50% y se siente a tirones.";

export function fmt(v, digits = 0, unit = "") {
  return v === null || v === undefined || Number.isNaN(v) ? "—" : Number(v).toFixed(digits) + unit;
}

export function fmtTime(seconds) {
  const s = Math.max(0, Math.round(seconds || 0));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

export function fmtDate(ms) {
  return new Date(ms).toLocaleString("es-AR", { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
}

export function summaryValue(row, summary) {
  const v = summary ? summary[row.key] : null;
  return row.time ? (v ? fmtTime(v) : "—") : fmt(v, row.digits, row.unit || "");
}

function tile(label, value, sub = "") {
  return (
    `<div class="perf-tile"><div class="perf-tile-label">${esc(label)}</div>` +
    `<div class="perf-tile-value">${esc(value)}</div>` +
    (sub ? `<div class="perf-tile-sub">${esc(sub)}</div>` : "") +
    `</div>`
  );
}

export function liveTilesHtml(s, live) {
  const ram = s.ram_used_mb != null ? `${(s.ram_used_mb / 1024).toFixed(1)} GB` : "—";
  const ramSub = s.ram_total_mb != null ? `de ${(s.ram_total_mb / 1024).toFixed(1)} GB` : "";
  const power = s.powered ? "—" : fmt(s.power_w, 2, " W");
  const powerSub = s.powered ? "enchufado: no se puede medir" : s.battery_pct != null ? `batería ${s.battery_pct}%` : "";
  return (
    tile("FPS ahora", fmt(s.fps, 0)) +
    tile("FPS promedio", fmt(live && live.avg_fps, 1)) +
    tile("1% low", fmt(live && live.low1_fps, 1), live ? `${live.frames} cuadros` : "") +
    tile("Ritmo", fmt(live && live.pacing_pct, 0, "%"), live && live.median_frametime_ms != null ? `a ±10% de ${fmt(live.median_frametime_ms, 1, " ms")}` : "") +
    tile("CPU", fmt(s.cpu_pct, 0, "%"), s.cpu_core_max_pct != null ? `núcleo máx. ${fmt(s.cpu_core_max_pct, 0, "%")}` : "") +
    tile("GPU", fmt(s.gpu_pct, 0, "%"), s.gpu_mhz != null ? `${s.gpu_mhz} MHz` : "") +
    tile("RAM", ram, ramSub) +
    tile("Consumo", power, powerSub) +
    tile("Temp. CPU", fmt(s.temp_cpu, 1, " °C"), s.temp_gpu != null ? `GPU ${fmt(s.temp_gpu, 1, " °C")}` : "") +
    tile("Temp. batería", fmt(s.temp_battery, 1, " °C"), s.temp_skin != null ? `carcasa ${fmt(s.temp_skin, 1, " °C")}` : "")
  );
}

export function summaryTilesHtml(sum) {
  return (
    tile("FPS promedio", fmt(sum.avg_fps, 1), sum.refresh_hz ? `pantalla a ${Math.round(sum.refresh_hz)} Hz` : "") +
    tile("1% low", fmt(sum.low1_fps, 1), `${sum.frames} cuadros`) +
    tile("Ritmo", fmt(sum.pacing_pct, 0, "%"), sum.median_frametime_ms != null ? `a ±10% de ${fmt(sum.median_frametime_ms, 1, " ms")} · desvío ${fmt(sum.frametime_sd_ms, 1, " ms")}` : "") +
    tile("Peor / mejor segundo", `${fmt(sum.min_fps, 0)} / ${fmt(sum.max_fps, 0)}`) +
    tile("CPU promedio", fmt(sum.avg_cpu_pct, 0, "%"), sum.cpu_cap_mhz ? `tope ${sum.cpu_cap_mhz} MHz` : "") +
    tile("GPU promedio", fmt(sum.avg_gpu_pct, 0, "%"), sum.gpu_cap_mhz ? `tope ${sum.gpu_cap_mhz} MHz` : "") +
    tile("Consumo promedio", fmt(sum.avg_power_w, 2, " W")) +
    tile("Temp. máx. CPU", fmt(sum.max_temp_cpu, 1, " °C"), sum.max_temp_gpu != null ? `GPU ${fmt(sum.max_temp_gpu, 1, " °C")}` : "") +
    tile("Temp. máx. batería", fmt(sum.max_temp_battery, 1, " °C"), sum.max_temp_skin != null ? `carcasa ${fmt(sum.max_temp_skin, 1, " °C")}` : "") +
    tile("Duración", fmtTime(sum.duration_s))
  );
}

// Where the live view says what it can and can't see right now.
export function statusHtml(s, device) {
  const pill = (ok, text) => `<span class="perf-pill${ok ? " perf-pill-ok" : ""}">${ok ? "✓" : "•"} ${esc(text)}</span>`;
  const wifi = device.serial.includes(":");
  return (
    pill(s.gamenative_running, s.gamenative_running ? "GameNative abierto" : "GameNative no está abierto") +
    pill(s.layer_found, s.layer_found ? "Juego detectado (FPS)" : "Buscando la pantalla del juego…") +
    pill(wifi, wifi ? "Conectado por Wi-Fi" : "Conectado por USB") +
    (s.powered ? pill(false, "Enchufado: sin medición de consumo") : "")
  );
}

// The table twin of every chart: each second, every metric.
export function samplesTableHtml(samples) {
  const cols = CHARTS.flatMap(c => c.series.map(s => ({ key: s.key, head: `${s.name} (${c.unit})` })));
  const head = `<tr><th>Tiempo</th>${cols.map(c => `<th>${esc(c.head)}</th>`).join("")}</tr>`;
  const rows = samples
    .map(s => `<tr><td>${fmtTime(s.t)}</td>${cols.map(c => `<td>${fmt(s[c.key], c.key.includes("temp") || c.key === "power_w" ? 1 : 0)}</td>`).join("")}</tr>`)
    .join("");
  return `<div class="perf-table-wrap"><table class="perf-table">${head}${rows}</table></div>`;
}

// "Ideas para mejorar" (android_perf_insights.rs). The level reads from the
// icon and the words; the border color only repeats it.
const LEVEL = { good: ["✓", "good"], info: ["ℹ", "info"], warn: ["⚠", "warn"] };

export function insightsHtml(list) {
  if (!list.length) return "";
  return (
    `<div class="detail-info-row"><span class="detail-info-label">Ideas para mejorar</span></div>` +
    list
      .map(i => {
        const [icon, level] = LEVEL[i.level] || LEVEL.info;
        return (
          `<div class="perf-insight perf-insight-${level}">` +
          `<div class="perf-insight-title">${icon} ${esc(i.title)}</div>` +
          `<div class="perf-insight-evidence">${esc(i.evidence)}</div>` +
          `<ul>${i.tips.map(t => `<li>${esc(t)}</li>`).join("")}</ul>` +
          `</div>`
        );
      })
      .join("") +
    `<p class="modal-hint">Son sugerencias a partir de lo medido: cambiá una cosa por vez, medí de nuevo y comparalas en "Sesiones".</p>`
  );
}
