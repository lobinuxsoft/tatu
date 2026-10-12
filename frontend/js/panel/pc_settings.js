import { esc } from "../utils.js";

// OptiScaler's image settings (PC tab): the same controls in the install form
// and, once installed, to change them without reinstalling. Each maps to a
// few keys of OptiScaler.ini (`tatu_upscalers::settings`); "del juego" /
// "automático" leave OptiScaler's default. Presets and FSR versions are the
// ones that very ini documents (`choices`). Everything here can also be
// changed in the game with INSERT.

const QUALITIES = [
  ["DLAA", "DLAA"],
  ["UltraQuality", "Ultra calidad"],
  ["Quality", "Calidad"],
  ["Balanced", "Equilibrado"],
  ["Performance", "Rendimiento"],
  ["UltraPerformance", "Ultra rendimiento"],
];
const SHARPNESS = [
  [0.2, "Suave (0.2)"],
  [0.4, "Media (0.4)"],
  [0.7, "Fuerte (0.7)"],
  [1.0, "Máxima (1.0)"],
];
const RATIOS = [
  [1.0, "Nativa, como DLAA (1.0×)"],
  [1.3, "1.3× — Ultra calidad"],
  [1.5, "1.5× — Calidad"],
  [1.7, "1.7× — Equilibrado"],
  [2.0, "2.0× — Rendimiento"],
  [3.0, "3.0× — Ultra rendimiento"],
];
const OUTPUT = [
  [1.5, "1.5×"],
  [2.0, "2.0×"],
];
const FPS = [30, 40, 45, 60, 90, 120, 144, 165, 240].map(v => [v, `${v} FPS`]);
const ANISOTROPY = [2, 4, 8, 16].map(v => [v, `${v}×`]);
// "Distinto por calidad": the DLSS preset select's own value, never sent.
export const BY_QUALITY = "by-quality";

// The ini's `{code, label}` choices as select options.
const fromIni = (list, label = l => l) => (list || []).map(o => [Number(o.code), label(o.label)]);

// OptiScaler's "latest" DLSS preset, which its menu can set.
const LATEST_PRESET = 0xffffff;

// A value the ini already has but the list doesn't (set from the in-game
// menu, or hidden for this GPU) stays selectable, under its own name.
function select(attrs, none, options, current, extra = "", labelOf = String) {
  const list = current != null && !options.some(([v]) => v === current) ? [...options, [current, labelOf(current)]] : options;
  return (
    `<select class="tatu-proton-select" ${attrs}><option value="">${none}</option>` +
    list.map(([v, label]) => `<option value="${v}"${v === current ? " selected" : ""}>${esc(label)}</option>`).join("") +
    extra +
    `</select>`
  );
}

// `s`: current settings (empty for a new install). `ctx`: `nvidia` (DLSS
// runs here), `fsr4` (an AMD card that may run FSR 4), `choices` (the ini's
// `dlss_presets`, `dlssd_presets`, `fsr_upscalers`, `fsr_fg`).
export function settingsHtml(s, { nvidia, fsr4, choices, byQuality: chosen = false }) {
  const row = (label, control, hint) =>
    `<label class="pc-api">${label} ${control}</label>` + (hint ? `<div class="modal-hint pc-setting-hint">${hint}</div>` : "");
  const heading = text => `<div class="pc-setting-group">${text}</div>`;
  const presets = fromIni(choices.dlss_presets, l => `Preset ${l}`);
  const presetLabel = v => (v === LATEST_PRESET ? "El más nuevo (latest)" : `Preset ${v}`);
  // A preset for every mode wins over per-mode ones (it's the one in effect).
  // `chosen`: picked in the form and kept through a re-render, before any
  // mode has one.
  const byQuality = s.dlss_preset == null && (chosen || Object.keys(s.dlss_quality_presets || {}).length > 0);
  // FSR 4 needs RDNA 4: elsewhere OptiScaler couldn't run it.
  const fsrOnly = list => (fsr4 ? list : list.filter(o => !/^FSR 4/.test(o.label)));
  const iniLabel = list => v => ((list || []).find(o => Number(o.code) === v) || { label: String(v) }).label;

  let html = `<div class="pc-settings">`;
  if (nvidia && presets.length) {
    html +=
      heading("DLSS") +
      row(
        "Preset:",
        select(
          'data-setting="dlss_preset"',
          "El del juego",
          presets,
          byQuality ? null : s.dlss_preset,
          `<option value="${BY_QUALITY}"${byQuality ? " selected" : ""}>Distinto por calidad…</option>`,
          presetLabel,
        ),
        "J en adelante son el modelo transformer: necesitan un DLSS 310 o más nuevo (con uno más viejo no hacen nada).",
      ) +
      `<div data-pc="dlss-qualities"${byQuality ? "" : ' style="display:none"'}>` +
      QUALITIES.map(([key, label]) =>
        row(
          `${label}:`,
          select(`data-quality="${key}"`, "El del juego", presets, (s.dlss_quality_presets || {})[key] ?? null, "", presetLabel),
        ),
      ).join("") +
      `</div>` +
      (choices.dlssd_presets && choices.dlssd_presets.length
        ? row(
            "Preset de Ray Reconstruction:",
            select(
              'data-setting="dlssd_preset"',
              "El del juego",
              fromIni(choices.dlssd_presets, l => `Preset ${l}`),
              s.dlssd_preset,
              "",
              presetLabel,
            ),
          )
        : "");
  }
  const fsrUpscalers = fromIni(fsrOnly(choices.fsr_upscalers || []));
  const fsrFg = fromIni(fsrOnly(choices.fsr_fg || []));
  if (fsrUpscalers.length || fsrFg.length) {
    html +=
      heading("FSR") +
      (fsrUpscalers.length
        ? row(
            "Versión:",
            select(
              'data-setting="fsr_upscaler"',
              "Automática (FSR 4 en RDNA 4, 3.1 en el resto)",
              fsrUpscalers,
              s.fsr_upscaler,
              "",
              iniLabel(choices.fsr_upscalers),
            ),
            "La que corre OptiScaler cuando el upscaler elegido es FSR.",
          )
        : "") +
      (fsrFg.length
        ? row("Frame generation:", select('data-setting="fsr_fg"', "Automática", fsrFg, s.fsr_fg, "", iniLabel(choices.fsr_fg)))
        : "");
  }
  html +=
    heading("Imagen") +
    row("Nitidez extra (RCAS):", select('data-setting="sharpness"', "Sin extra", SHARPNESS, s.sharpness)) +
    row(
      "Resolución interna fija:",
      select('data-setting="upscale_ratio"', "La de la calidad que elijas en el juego", RATIOS, s.upscale_ratio),
      "Ignora la calidad que elijas en el juego. «Nativa» usa el upscaler como antialiasing, sin bajar la resolución.",
    ) +
    row(
      "Output scaling:",
      select('data-setting="output_scaling"', "No", OUTPUT, s.output_scaling),
      "Escala a más que tu pantalla y reduce después: más nítido y más costoso. DirectX 12 (y DX11 a través de DX12).",
    ) +
    row(
      "Límite de FPS:",
      select('data-setting="fps_limit"', "Sin límite", FPS, s.fps_limit),
      "Con Reflex: el juego tiene que tenerlo (en AMD e Intel lo da el fakenvapi que trae OptiScaler).",
    ) +
    row("Filtrado anisotrópico:", select('data-setting="anisotropy"', "El del juego", ANISOTROPY, s.anisotropy)) +
    `</div>`;
  return html;
}

// Wire what the controls under `el` need after being rendered.
export function installSettings(el) {
  const preset = el.querySelector('[data-setting="dlss_preset"]');
  const qualities = el.querySelector('[data-pc="dlss-qualities"]');
  if (preset && qualities) preset.onchange = () => (qualities.style.display = preset.value === BY_QUALITY ? "" : "none");
}

// The settings as the controls under `el` have them, in the shape Rust
// expects, over `base`: a setting with no control here (DLSS on a GPU that
// doesn't run it) keeps what the ini had.
export function readSettings(el, base = {}) {
  const s = { ...base };
  el.querySelectorAll("[data-setting]").forEach(control => {
    s[control.dataset.setting] = control.value === "" || control.value === BY_QUALITY ? null : Number(control.value);
  });
  const preset = el.querySelector('[data-setting="dlss_preset"]');
  if (preset) {
    s.dlss_quality_presets = {};
    if (preset.value === BY_QUALITY) {
      el.querySelectorAll("[data-quality]").forEach(control => {
        if (control.value !== "") s.dlss_quality_presets[control.dataset.quality] = Number(control.value);
      });
    }
  }
  return s;
}

// `s` picked against the `from` choices, for the `to` choices: each code
// through its name (the same code can be another preset or FSR in another
// build); what `to` doesn't name goes back to the default.
export function remapSettings(s, from, to) {
  const map = (key, v) => {
    if (v == null) return null;
    const label = ((from[key] || []).find(o => Number(o.code) === v) || {}).label;
    const same = label != null && (to[key] || []).find(o => o.label === label);
    return same ? Number(same.code) : null;
  };
  const byQuality = Object.entries(s.dlss_quality_presets || {})
    .map(([quality, v]) => [quality, map("dlss_presets", v)])
    .filter(([, v]) => v != null);
  return {
    ...s,
    dlss_preset: map("dlss_presets", s.dlss_preset),
    dlss_quality_presets: Object.fromEntries(byQuality),
    dlssd_preset: map("dlssd_presets", s.dlssd_preset),
    fsr_upscaler: map("fsr_upscalers", s.fsr_upscaler),
    fsr_fg: map("fsr_fg", s.fsr_fg),
  };
}

// The choices as `optiscaler_prepare` / `optiscaler_custom_options` send them.
export function choicesFromOptions(options) {
  return {
    dlss_presets: options.DlssPreset,
    dlssd_presets: options.DlssdPreset,
    fsr_upscalers: options.FsrUpscaler,
    fsr_fg: options.FsrFg,
  };
}
