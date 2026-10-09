import { invoke } from "../tauri.js";
import { esc } from "../utils.js";

// "Mejor configuración" (GameNative tab): the user's own record of the
// GameNative container setup that works best for this game, with a name and
// notes for the in-game settings that go with it. Only written down — never
// applied to GameNative automatically.
//
// Keys are GameNative's own container settings (Winlator's `ContainerData`,
// the same JSON its "Export config" writes), so a saved config could be
// handed back to its "Import config" later. Placeholders are GameNative's
// defaults; `options` are the values its own pickers offer, as suggestions
// only (a <datalist>) — GameNative adds drivers and versions over time.
export const CONFIG_FIELDS = [
  { key: "containerVariant", label: "Variante", placeholder: "glibc", options: ["glibc", "bionic"] },
  { key: "wineVersion", label: "Wine / Proton", placeholder: "wine-9.2-x86_64", options: ["wine-9.2-x86_64", "proton-9.0-arm64ec"] },
  { key: "graphicsDriver", label: "Driver gráfico", placeholder: "vortek", options: ["vortek", "turnip", "virgl", "adreno", "wrapper"] },
  { key: "graphicsDriverVersion", label: "Versión del driver" },
  { key: "dxwrapper", label: "DX wrapper", placeholder: "dxvk", options: ["dxvk", "vkd3d", "wined3d", "cnc-ddraw"] },
  { key: "dxwrapperConfig", label: "Config del DX wrapper", placeholder: "ej. version=2.6.1-gplasync" },
  { key: "emulator", label: "Emulador", placeholder: "FEXCore", options: ["FEXCore", "Box64"] },
  { key: "fexcorePreset", label: "Preset FEXCore", placeholder: "INTERMEDIATE", options: ["STABILITY", "COMPATIBILITY", "INTERMEDIATE", "PERFORMANCE", "EXTREME"] },
  { key: "box64Preset", label: "Preset Box64", placeholder: "COMPATIBILITY", options: ["STABILITY", "COMPATIBILITY", "INTERMEDIATE", "PERFORMANCE", "DENUVO"] },
  { key: "screenSize", label: "Resolución", placeholder: "1280x720" },
  { key: "cpuList", label: "CPUs", placeholder: "ej. 0,1,2,3,4,5,6,7" },
  { key: "audioDriver", label: "Audio", placeholder: "pulseaudio", options: ["pulseaudio", "alsa"] },
  { key: "envVars", label: "Variables de entorno", wide: true },
  { key: "execArgs", label: "Argumentos de lanzamiento", wide: true },
  { key: "showFPS", label: "Mostrar FPS", bool: true },
  { key: "lsfgEnabled", label: "Frame generation (LSFG)", bool: true },
];

export function configSectionHtml() {
  return (
    `<div class="detail-info-row"><span class="detail-info-label">Mejor configuración</span></div>` +
    `<div id="gnConfig"></div>`
  );
}

// `source` + `id` key the saved config ("steam"/"non_steam", same as the
// rest of the tab).
export async function installConfigSection(source, id) {
  const el = document.getElementById("gnConfig");
  if (!el) return;
  const ctx = { source, id, el, config: null };
  try {
    ctx.config = await invoke("gamenative_get_config", { source, id });
  } catch (e) {
    el.innerHTML = `<div class="cartridge-warn">${esc(String(e))}</div>`;
    return;
  }
  showView(ctx);
}

function showView(ctx) {
  const c = ctx.config;
  if (!c) {
    ctx.el.innerHTML =
      `<div class="modal-hint">Todavía no guardaste una configuración de GameNative para este juego.</div>` +
      `<button class="cartridge-btn-secondary" data-gn="edit">＋ Agregar configuración</button>`;
  } else {
    ctx.el.innerHTML = configValuesHtml(c) + `<button class="cartridge-btn-secondary" data-gn="edit">✎ Editar</button>`;
  }
  ctx.el.querySelector('[data-gn="edit"]').onclick = () => showEdit(ctx);
}

function fieldInput(f, value) {
  if (f.bool) {
    return (
      `<label class="gn-field gn-check"><input type="checkbox" data-key="${f.key}"${value ? " checked" : ""}>` +
      `${esc(f.label)}</label>`
    );
  }
  const list = f.options ? ` list="gn-opt-${f.key}"` : "";
  const datalist = f.options
    ? `<datalist id="gn-opt-${f.key}">${f.options.map(o => `<option value="${esc(o)}">`).join("")}</datalist>`
    : "";
  return (
    `<label class="gn-field${f.wide ? " gn-field-wide" : ""}">${esc(f.label)}` +
    `<input type="text" class="gn-input" data-key="${f.key}" value="${esc(value === undefined ? "" : String(value))}"` +
    ` placeholder="${esc(f.placeholder || "")}"${list}>${datalist}</label>`
  );
}

// Read-only: name, the values that are set, notes. Shared with the
// performance window's sessions (each keeps the config it ran with).
export function configValuesHtml(c) {
  const values = CONFIG_FIELDS.filter(f => c.container[f.key] !== undefined)
    .map(f => `<div class="gn-field${f.wide ? " gn-field-wide" : ""}"><span>${esc(f.label)}</span><span class="gn-value">${esc(configValueText(f, c.container[f.key]))}</span></div>`)
    .join("");
  return (
    `<div class="gn-config-name">${esc(c.name || "Sin nombre")}</div>` +
    (values ? `<div class="gn-grid">${values}</div>` : "") +
    (c.notes ? `<div class="gn-notes-view"><span>Notas de configuración del juego</span>${esc(c.notes)}</div>` : "")
  );
}

export function configValueText(field, value) {
  if (value === undefined) return "—";
  return field.bool ? (value ? "Sí" : "No") : String(value);
}

// The editable form (no buttons) — read back with `readConfigForm`.
export function configFormHtml(config) {
  const c = config || { name: "", container: {}, notes: "" };
  return (
    `<label class="gn-field gn-field-wide">Nombre de esta configuración` +
    `<input type="text" class="gn-input" data-gn="name" value="${esc(c.name)}" placeholder="ej. 60 fps estable"></label>` +
    `<div class="modal-hint">Los valores de GameNative (lo que dejes vacío no se guarda). ` +
    `Tatu no los aplica solo: es tu registro.</div>` +
    `<div class="gn-grid">${CONFIG_FIELDS.map(f => fieldInput(f, c.container[f.key])).join("")}</div>` +
    `<label class="gn-field gn-field-wide">Notas de configuración del juego` +
    `<textarea class="gn-input gn-notes" data-gn="notes" placeholder="ej. Gráficos en Bajo, V-Sync apagado, límite de 30 fps">${esc(c.notes)}</textarea></label>`
  );
}

function showEdit(ctx) {
  ctx.el.innerHTML =
    configFormHtml(ctx.config) +
    `<div class="cartridge-actions">` +
    `<button class="cartridge-btn" data-gn="save">Guardar</button>` +
    `<button class="cartridge-btn-secondary" data-gn="cancel">Cancelar</button>` +
    (ctx.config ? `<button class="cartridge-btn-secondary" data-gn="delete">Borrar</button>` : "") +
    `</div>` +
    `<div data-gn="msg"></div>`;

  const q = sel => ctx.el.querySelector(sel);
  q('[data-gn="cancel"]').onclick = () => showView(ctx);
  q('[data-gn="save"]').onclick = () => save(ctx, readConfigForm(ctx.el));
  if (ctx.config) {
    q('[data-gn="delete"]').onclick = () => save(ctx, { name: "", container: {}, notes: "" });
  }
}

// Blank fields and unchecked boxes are left out rather than stored as ""
// or false — "not set" is different from "set to off".
export function readConfigForm(el) {
  const container = {};
  el.querySelectorAll("[data-key]").forEach(input => {
    if (input.type === "checkbox") {
      if (input.checked) container[input.dataset.key] = true;
    } else if (input.value.trim()) {
      container[input.dataset.key] = input.value.trim();
    }
  });
  return {
    name: el.querySelector('[data-gn="name"]').value.trim(),
    container,
    notes: el.querySelector('[data-gn="notes"]').value.trim(),
  };
}

async function save(ctx, config) {
  try {
    await invoke("gamenative_save_config", { source: ctx.source, id: ctx.id, config });
    const empty = !config.name && !config.notes && Object.keys(config.container).length === 0;
    ctx.config = empty ? null : config;
    showView(ctx);
  } catch (e) {
    ctx.el.querySelector('[data-gn="msg"]').innerHTML =
      `<div class="cartridge-warn">No pude guardar: ${esc(String(e))}</div>`;
  }
}
