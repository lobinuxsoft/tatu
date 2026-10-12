import { esc } from "../utils.js";
import { fmt, fmtTime } from "./metrics.js";

// Line chart as inline SVG, drawn at the card's real pixel width. One unit
// per chart; 2px round lines, hairline recessive grid, end dots with a ring
// in the surface color; a legend for 2+ series (a single series is named by
// the title). A crosshair snaps to the nearest second and one tooltip lists
// every series there — on hover and on keyboard focus (arrow keys). An
// unavailable reading breaks the line instead of drawing a fake 0.

const H = 180;
const M = { top: 10, right: 14, bottom: 22, left: 42 };
const NS = "http://www.w3.org/2000/svg";

// Series colors: validated categorical slots (perf.css `--viz-*`), assigned
// by the series' fixed position, never by what happens to have data.
export const slotColor = i => `var(--viz-${(i % 4) + 1})`;

// `rows`: objects with `t` (seconds) and a value per series key.
// `opts.colors`: explicit per-series colors (comparison keeps each
// session's own slot); default = slot by position.
export function renderChart(el, chart, rows, opts = {}) {
  const colors = chart.series.map((s, i) => (opts.colors && opts.colors[i]) || slotColor(i));
  const present = chart.series
    .map((s, i) => ({ ...s, color: colors[i] }))
    .filter(s => rows.some(r => r[s.key] !== null && r[s.key] !== undefined));

  let html = `<div class="perf-chart-title">${esc(chart.title)} <span class="perf-chart-unit">(${esc(chart.unit)})</span></div>`;
  if (!present.length) {
    el.innerHTML = html + `<div class="perf-empty">${rows.length ? "No disponible en este dispositivo" : "Esperando datos…"}</div>`;
    return;
  }
  if (present.length > 1) {
    html +=
      `<div class="perf-legend">` +
      present.map(s => `<span><i class="perf-key" style="background:${s.color}"></i>${esc(s.name)}</span>`).join("") +
      `</div>`;
  }
  el.innerHTML = html;

  const svg = document.createElementNS(NS, "svg");
  svg.setAttribute("tabindex", "0");
  svg.setAttribute("role", "img");
  svg.setAttribute("aria-label", `${chart.title} (${chart.unit})`);
  el.appendChild(svg);
  const W = Math.max(svg.getBoundingClientRect().width, 240);
  svg.setAttribute("viewBox", `0 0 ${W} ${H}`);

  const t0 = rows[0].t;
  const t1 = Math.max(rows[rows.length - 1].t, t0 + 1);
  let peak = 0;
  for (const r of rows) for (const s of present) if (r[s.key] != null) peak = Math.max(peak, r[s.key]);
  const { max: yMax, step } = niceScale(chart.max !== undefined ? chart.max : peak);
  const pw = W - M.left - M.right;
  const ph = H - M.top - M.bottom;
  const x = t => M.left + ((t - t0) / (t1 - t0)) * pw;
  const y = v => M.top + ph - (v / yMax) * ph;

  let body = `<g class="perf-grid">`;
  for (let v = 0; v <= yMax + 1e-9; v += step) {
    body += `<line x1="${M.left}" x2="${W - M.right}" y1="${y(v)}" y2="${y(v)}"/>`;
  }
  body += `</g><g class="perf-axis">`;
  for (let v = 0; v <= yMax + 1e-9; v += step) {
    body += `<text x="${M.left - 6}" y="${y(v) + 3}" text-anchor="end">${fmt(v, step < 1 ? 1 : 0)}</text>`;
  }
  const xTicks = Math.min(5, Math.max(1, Math.floor(pw / 90)));
  for (let i = 0; i <= xTicks; i++) {
    const t = t0 + ((t1 - t0) * i) / xTicks;
    const anchor = i === 0 ? "start" : i === xTicks ? "end" : "middle";
    body += `<text x="${x(t)}" y="${H - 6}" text-anchor="${anchor}">${fmtTime(t)}</text>`;
  }
  body += `</g>`;

  for (const s of present) {
    let d = "";
    let pen = false;
    let last = null;
    for (const r of rows) {
      const v = r[s.key];
      if (v === null || v === undefined) {
        pen = false;
        continue;
      }
      d += `${pen ? "L" : "M"}${x(r.t).toFixed(1)},${y(Math.min(v, yMax)).toFixed(1)}`;
      pen = true;
      last = r;
    }
    body += `<path class="perf-line" d="${d}" stroke="${s.color}"/>`;
    if (last) {
      body += `<circle class="perf-dot" cx="${x(last.t)}" cy="${y(Math.min(last[s.key], yMax))}" r="4" fill="${s.color}"/>`;
    }
  }
  body +=
    `<line class="perf-cross" y1="${M.top}" y2="${M.top + ph}" visibility="hidden"/>` +
    `<rect x="${M.left}" y="${M.top}" width="${pw}" height="${ph}" fill="transparent"/>`;
  svg.innerHTML = body;

  installCrosshair(el, svg, rows, present, x, chart.unit);
}

// 1-2-5 steps; four gridlines-ish; the top gridline sits at or above the peak.
function niceScale(peak) {
  const raw = Math.max(peak, 1) / 4;
  const mag = Math.pow(10, Math.floor(Math.log10(raw)));
  const step = [1, 2, 2.5, 5, 10].map(m => m * mag).find(s => s >= raw);
  return { step, max: Math.ceil(Math.max(peak, 1) / step) * step };
}

function installCrosshair(el, svg, rows, series, x, unit) {
  const cross = svg.querySelector(".perf-cross");
  const tip = document.createElement("div");
  tip.className = "perf-tip";
  tip.hidden = true;
  el.appendChild(tip);
  let index = rows.length - 1;

  const show = i => {
    index = Math.max(0, Math.min(rows.length - 1, i));
    const r = rows[index];
    const px = x(r.t);
    cross.setAttribute("x1", px);
    cross.setAttribute("x2", px);
    cross.setAttribute("visibility", "visible");

    // Values lead, names follow; built with textContent — names are data.
    tip.replaceChildren();
    const time = document.createElement("div");
    time.className = "perf-tip-time";
    time.textContent = fmtTime(r.t);
    tip.appendChild(time);
    for (const s of series) {
      const row = document.createElement("div");
      row.className = "perf-tip-row";
      const key = document.createElement("i");
      key.className = "perf-key";
      key.style.background = s.color;
      const value = document.createElement("b");
      value.textContent = fmt(r[s.key], unit === "W" || unit === "°C" ? 1 : 0, ` ${unit}`);
      const name = document.createElement("span");
      name.textContent = s.name;
      row.append(key, value, name);
      tip.appendChild(row);
    }
    tip.hidden = false;
    const box = svg.getBoundingClientRect();
    const scale = box.width / svg.viewBox.baseVal.width;
    const left = svg.offsetLeft + px * scale;
    tip.style.top = `${svg.offsetTop + 8}px`;
    tip.style.left = left > box.width / 2 ? `${left - tip.offsetWidth - 10}px` : `${left + 10}px`;
  };
  const hide = () => {
    tip.hidden = true;
    cross.setAttribute("visibility", "hidden");
  };
  const nearest = clientX => {
    const box = svg.getBoundingClientRect();
    const vx = ((clientX - box.left) / box.width) * svg.viewBox.baseVal.width;
    let best = 0;
    for (let i = 1; i < rows.length; i++) {
      if (Math.abs(x(rows[i].t) - vx) < Math.abs(x(rows[best].t) - vx)) best = i;
    }
    return best;
  };

  svg.addEventListener("pointermove", e => show(nearest(e.clientX)));
  svg.addEventListener("pointerleave", hide);
  svg.addEventListener("focus", () => show(index));
  svg.addEventListener("blur", hide);
  svg.addEventListener("keydown", e => {
    if (e.key === "ArrowLeft") show(index - 1);
    else if (e.key === "ArrowRight") show(index + 1);
    else return;
    e.preventDefault();
  });
}
