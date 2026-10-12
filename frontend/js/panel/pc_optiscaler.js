import { invoke, listen } from "../tauri.js";
import { esc, formatBytes, downloadToken } from "../utils.js";
import {
  settingsHtml,
  installSettings,
  readSettings,
  choicesFromOptions,
  remapSettings,
  BY_QUALITY,
} from "./pc_settings.js";

// OptiScaler (PC tab): install the pinned release into the game — every game
// file it replaces backed up first — or revert it. The download happens once
// for every game; which upscaler codes exist comes from the ini that will be
// installed (they changed between OptiScaler versions).

// Proton already loads these two native (DXVK's dxgi, vkd3d-proton's d3d12):
// no launch option needed under those names.
const PROTON_NATIVE = ["dxgi.dll", "d3d12.dll"];
const API_KEYS = [
  ["Dx12Upscaler", "DirectX 12"],
  ["Dx11Upscaler", "DirectX 11"],
  ["VulkanUpscaler", "Vulkan"],
];

// Frame generation, in words. Codes come from the ini; anything unknown
// shows as the ini's own text.
const FG_INPUT_LABEL = {
  dlssg: "El botón de DLSS Frame Generation del juego",
  fsrfg: "El FSR Frame Generation del juego",
  fsrfg30: "El FSR 3.0 Frame Generation del juego",
  upscaler: "El upscaler, en cualquier juego (aunque no traiga frame gen)",
  nukems: "El DLSS Frame Generation del juego, vía dlssg-to-fsr3",
};
const FG_OUTPUT_LABEL = {
  fsrfg: "FSR Frame Generation (FSR 4 en AMD RDNA4, FSR 3.1 en el resto)",
  xefg: "XeSS Frame Generation (pensado para Intel Arc)",
  nukems: "dlssg-to-fsr3 (FSR 3)",
};
// The installed view's one-liner: "in → out" from the patch record.
const FG_INPUT_SHORT = {
  dlssg: "del DLSS Frame Generation del juego",
  fsrfg: "del FSR Frame Generation del juego",
  fsrfg30: "del FSR 3.0 Frame Generation del juego",
  upscaler: "del upscaler",
  nukems: "del DLSS Frame Generation del juego",
};
const FG_OUTPUT_SHORT = { fsrfg: "FSR Frame Generation", xefg: "XeSS Frame Generation", nukems: "dlssg-to-fsr3" };

// What FSR 4 means on this PC: RDNA 4 (and whatever comes after) runs it;
// an AMD card of unknown generation might. Only the GPU games run on counts
// — listed first —: a laptop's AMD iGPU next to an RTX doesn't.
export function fsr4Note(gpus) {
  const g = gpus[0];
  if (!g || g.vendor !== "amd") return "";
  return g.rdna >= 4 ? "sure" : g.rdna ? "" : "maybe";
}

// What OptiScaler can switch to on this PC, in words.
function upscalerTargets(d) {
  const fsr4 = fsr4Note(d.gpus);
  const list = ["FSR 3.1", "XeSS"];
  if (d.nvidia) list.push("DLSS");
  if (fsr4 === "sure") list.push("FSR 4 (tu placa lo corre)");
  return (
    list.slice(0, -1).join(", ") +
    " o " +
    list[list.length - 1] +
    (fsr4 === "maybe" ? ", y FSR 4 si la placa es AMD RDNA 4" : "")
  );
}

export function optiscalerSectionHtml() {
  return `<div class="detail-info-row"><span class="detail-info-label">OptiScaler</span></div><div data-pc="opti"></div>`;
}

export function installOptiscalerSection(ctx) {
  const el = ctx.el.querySelector('[data-pc="opti"]');
  if (ctx.d.optiscaler) renderInstalled(ctx, el);
  else renderIdle(ctx, el);
}

function renderIdle(ctx, el) {
  ctx.editing = null;
  const d = ctx.d;
  const blocked =
    !d.exe && !d.target_custom
      ? `No encontré el ejecutable de Windows del juego. Si no es la versión nativa de Linux, elegí su carpeta con "Cambiar…".`
      : d.linux && d.linux.steam && d.linux.launch_options_error
        ? `No pude leer las opciones de lanzamiento de Steam (${d.linux.launch_options_error}): hacen falta para ver ` +
          `si GE-Proton ya carga su propio OptiScaler.`
        : d.linux && d.linux.proton_optiscaler
        ? "Este juego ya usa el OptiScaler que trae GE-Proton (PROTON_USE_OPTISCALER en sus opciones de lanzamiento)."
        : d.legacy_optiscaler
          ? `Hay un OptiScaler viejo instalado como ${d.legacy_optiscaler} en la carpeta del ejecutable: quitalo antes, ` +
            `o se cargarían los dos.`
          : !d.default_proxy
            ? "Todos los nombres que OptiScaler puede usar ya están ocupados en la carpeta del juego."
            : "";
  el.innerHTML =
    `<p class="modal-hint">Cambia el upscaler que trae el juego — DLSS, FSR 2/3/3.1 o XeSS — por otro: ${upscalerTargets(d)}` +
    `. Por ejemplo ${d.nvidia ? "DLSS en un juego que solo trae FSR o XeSS" : "FSR en un juego que solo trae DLSS"}. ` +
    `También agrega frame generation. Se descarga de su página oficial (versión ${esc(d.package_version)}). Cada ` +
    `archivo del juego que reemplace queda respaldado y "Revertir" lo deja como estaba.</p>` +
    (d.foreign_optiscaler
      ? `<div class="cartridge-guide">Ya hay un OptiScaler instalado a mano como <b>${esc(d.foreign_optiscaler)}</b>. ` +
        `Instalando desde acá se reemplaza, y queda respaldado.</div>`
      : "") +
    leftoverLaunchHtml(d) +
    (blocked
      ? `<div class="cartridge-warn">${esc(blocked)}</div>`
      : `<div class="cartridge-actions"><button class="cartridge-btn" data-pc="start">⚙ Instalar OptiScaler…</button></div>`);
  const start = el.querySelector('[data-pc="start"]');
  if (start) start.onclick = () => prepare(ctx, el);
  installLaunchButton(ctx, el);
}

// Downloads (first time) and extracts the release; answers the upscaler codes.
async function prepare(ctx, el) {
  ctx.editing = { installed: false };
  el.innerHTML =
    `<div class="ach-progress-wrap"><div class="ach-progress-bar" data-pc="bar" style="width:0%"></div>` +
    `<div class="ach-progress-text" data-pc="text">Preparando OptiScaler...</div></div>` +
    `<div class="cartridge-actions"><button class="cartridge-btn-secondary" data-pc="cancel">Cancelar</button></div>`;
  const token = downloadToken();
  el.querySelector('[data-pc="cancel"]').onclick = e => {
    e.target.disabled = true;
    invoke("pc_cancel_download", { token }).catch(() => {});
  };
  const unlisten = await listen("optiscaler_progress", e => {
    const { done, total } = e.payload || {};
    const bar = el.querySelector('[data-pc="bar"]');
    const text = el.querySelector('[data-pc="text"]');
    if (bar) bar.style.width = (total ? Math.min(100, Math.round((done / total) * 100)) : 0) + "%";
    if (text) text.textContent = done >= total ? "Descomprimiendo..." : `Descargando: ${formatBytes(done)} / ${formatBytes(total)}`;
  });
  try {
    renderForm(ctx, el, await invoke("optiscaler_prepare", { cancelToken: token }));
  } catch (e) {
    el.innerHTML =
      `<div class="cartridge-warn">${esc(String(e))}</div>` +
      `<div class="cartridge-actions"><button class="cartridge-btn-secondary" data-pc="back">Volver</button></div>`;
    el.querySelector('[data-pc="back"]').onclick = () => renderIdle(ctx, el);
  } finally {
    unlisten();
  }
}

function renderForm(ctx, el, options) {
  const d = ctx.d;
  const form = { custom: null, options };
  const proxyOptions = d.proxies
    .map(p => {
      const usable = d.foreign_optiscaler ? p.name === d.foreign_optiscaler : !p.taken;
      const note = !usable && p.taken ? ` — ocupado (${p.taken})` : "";
      return `<option value="${esc(p.name)}"${usable ? "" : " disabled"}${p.name === d.default_proxy ? " selected" : ""}>${esc(p.name)}${esc(note)}</option>`;
    })
    .join("");

  el.innerHTML =
    `<div class="pc-form">` +
    `<label>Se carga como <select class="tatu-proton-select" data-pc="proxy">${proxyOptions}</select></label>` +
    `<div class="modal-hint" data-pc="proxy-hint"></div>` +
    `<details class="pc-details"><summary>Upscaler a usar (opcional: también se elige en el juego con INSERT)</summary>` +
    `<div data-pc="upscalers"></div></details>` +
    `<div data-pc="fg"></div>` +
    // No upscaler DLL found: FSR 2/3 is likely compiled into the exe.
    `<label class="pc-check"><input type="checkbox" data-pc="fsr-pattern"${d.scan.upscalers.length ? "" : " checked"}> ` +
    `Buscar FSR 2/3 dentro del ejecutable</label>` +
    `<div class="modal-hint">Para juegos que traen FSR 2 o 3 compilado en el .exe, sin DLL. OptiScaler lo busca al ` +
    `arrancar, así que el juego tarda un poco más en cargar.</div>` +
    (!d.nvidia
      ? `<label class="pc-check"><input type="checkbox" data-pc="dlss-inputs" checked> Usar las entradas de DLSS (spoofing)</label>` +
        `<div class="modal-hint">Hace falta para usar FSR/XeSS en juegos que solo traen DLSS. En un juego con FSR o XeSS propio podés destildarlo.</div>`
      : "") +
    `<details class="pc-details"><summary>Ajustes de imagen (opcional: también se cambian después)</summary>` +
    `<div data-pc="settings-box"></div>` +
    `</details>` +
    `<fieldset class="pc-source"><legend>Versión:</legend>` +
    `<label><input type="radio" name="pcSource" value="release" checked> OptiScaler ${esc(d.package_version)} oficial</label> ` +
    `<label><input type="radio" name="pcSource" value="custom"> Mi build</label> ` +
    `<button class="cartridge-btn-secondary" data-pc="pick-dll" style="display:none">Elegir OptiScaler.dll…</button> ` +
    `<span class="pc-path" data-pc="custom-path"></span></fieldset>` +
    (d.scan.anti_cheat.length
      ? `<label class="pc-check"><input type="checkbox" data-pc="anticheat"> Lo juego sin conexión y entiendo el riesgo del anti-cheat (${esc(d.scan.anti_cheat.join(", "))})</label>`
      : "") +
    `</div>` +
    `<div data-pc="form-msg" aria-live="polite"></div>` +
    `<div class="cartridge-actions">` +
    `<button class="cartridge-btn" data-pc="install">Instalar</button>` +
    `<button class="cartridge-btn-secondary" data-pc="cancel">Cancelar</button>` +
    `</div>`;

  const $ = s => el.querySelector(s);
  const formMsg = html => ($('[data-pc="form-msg"]').innerHTML = html);
  const renderUpscalers = () => {
    $('[data-pc="upscalers"]').innerHTML = API_KEYS.filter(([key]) => (form.options[key] || []).length)
      .map(
        ([key, label]) =>
          `<label class="pc-api">${label} <select class="tatu-proton-select" data-key="${key}"><option value="auto">Automático (lo decide OptiScaler)</option>` +
          form.options[key].map(o => `<option value="${esc(o.code)}">${esc(o.label)}</option>`).join("") +
          `</select></label>`,
      )
      .join("");
  };
  // The game's own frame gen is the best source; any upscaler works too, but
  // the game's UI can then smear into generated frames.
  const renderFrameGen = () => {
    const inputs = (form.options.FGInput || []).filter(o => o.code !== "nofg");
    const outputs = (form.options.FGOutput || []).filter(o => o.code !== "nofg");
    const box = $('[data-pc="fg"]');
    if (!inputs.length || !outputs.length) {
      box.innerHTML = "";
      return;
    }
    const kinds = d.scan.upscalers.map(u => u.kind);
    const has = code => inputs.some(o => o.code === code);
    const defaultInput =
      (kinds.includes("dlssg") || kinds.includes("streamline")) && has("dlssg")
        ? "dlssg"
        : kinds.includes("fsrfg") && has("fsrfg")
          ? "fsrfg"
          : has("upscaler")
            ? "upscaler"
            : inputs[0].code;
    const select = (name, list, labels, selected) =>
      `<select class="tatu-proton-select" data-pc="${name}">` +
      list
        .map(o => `<option value="${esc(o.code)}"${o.code === selected ? " selected" : ""}>${esc(labels[o.code] || o.label)}</option>`)
        .join("") +
      `</select>`;
    box.innerHTML =
      `<label class="pc-check"><input type="checkbox" data-pc="fg-on"> Activar frame generation</label>` +
      `<div data-pc="fg-opts" style="display:none">` +
      `<label class="pc-api">Toma los cuadros de: ${select("fg-input", inputs, FG_INPUT_LABEL, defaultInput)}</label>` +
      `<label class="pc-api">Los genera: ${select("fg-output", outputs, FG_OUTPUT_LABEL, outputs.some(o => o.code === "fsrfg") ? "fsrfg" : outputs[0].code)}</label>` +
      `<div class="modal-hint" data-pc="fg-hint"></div></div>`;
    const hint = () => {
      const input = $('[data-pc="fg-input"]').value;
      $('[data-pc="fg-hint"]').textContent =
        (input === "dlssg"
          ? "El juego muestra su opción de DLSS Frame Generation aunque la placa no lo soporte: OptiScaler se presenta como RTX 4090 ante Streamline. Activala en el juego. "
          : input === "upscaler"
            ? "Sin frame gen propio del juego, la interfaz puede verse mal en los cuadros generados: si pasa, probá el Hudfix del menú de OptiScaler (INSERT). " +
              // FSR 3.1 always ships its frame gen; whether the game uses it
              // only shows in its options.
              (kinds.includes("fsr3") && has("fsrfg")
                ? "Si el juego tiene su propia opción de FSR Frame Generation, elegí esa fuente: se ve mejor. "
                : "")
            : "") + "Suma algo de latencia: rinde mejor con 60 FPS o más de base.";
    };
    $('[data-pc="fg-on"]').onchange = e => ($('[data-pc="fg-opts"]').style.display = e.target.checked ? "" : "none");
    $('[data-pc="fg-input"]').onchange = hint;
    hint();
  };
  const proxyHint = () => {
    const proxy = $('[data-pc="proxy"]').value;
    const stem = proxy.replace(/\.dll$/i, "");
    $('[data-pc="proxy-hint"]').textContent = !d.linux
      ? "El juego lo carga solo, sin configurar nada."
      : PROTON_NATIVE.includes(proxy)
        ? "En Linux, Proton ya carga este nombre desde la carpeta del juego: no hace falta tocar las opciones de lanzamiento."
        : d.linux.steam
          ? `Con este nombre Proton necesita WINEDLLOVERRIDES="${stem}=n,b": Tatu lo agrega a las opciones de lanzamiento de Steam (con Steam cerrado).`
          : `Con este nombre hay que agregar WINEDLLOVERRIDES="${stem}=n,b" en las variables de entorno del lanzador del juego.`;
  };
  // Keeps what was picked when a custom build brings other choices — by
  // name: the same code can mean another preset or FSR in another build.
  let shownChoices = null;
  const renderSettings = () => {
    const box = $('[data-pc="settings-box"]');
    const choices = choicesFromOptions(form.options);
    const preset = box.querySelector('[data-setting="dlss_preset"]');
    const byQuality = !!preset && preset.value === BY_QUALITY;
    const picked = shownChoices ? remapSettings(readSettings(box), shownChoices, choices) : {};
    box.innerHTML = settingsHtml(picked, { nvidia: d.nvidia, fsr4: !!fsr4Note(d.gpus), choices, byQuality });
    installSettings(box);
    shownChoices = choices;
  };
  renderUpscalers();
  renderFrameGen();
  renderSettings();
  proxyHint();
  $('[data-pc="proxy"]').onchange = proxyHint;

  el.querySelectorAll('input[name="pcSource"]').forEach(radio => {
    radio.onchange = () => {
      const custom = radio.value === "custom" && radio.checked;
      if (radio.checked) $('[data-pc="pick-dll"]').style.display = custom ? "" : "none";
      if (!custom && radio.checked) {
        form.custom = null;
        form.options = options;
        $('[data-pc="custom-path"]').textContent = "";
        renderUpscalers();
        renderFrameGen();
        renderSettings();
      }
    };
  });
  const pickDll = $('[data-pc="pick-dll"]');
  pickDll.onclick = async () => {
    pickDll.disabled = true;
    const dll = await invoke("pc_pick_dll", { startIn: null }).catch(() => null);
    pickDll.disabled = false;
    if (!dll) return;
    try {
      form.options = await invoke("optiscaler_custom_options", { dll });
      form.custom = dll;
      $('[data-pc="custom-path"]').textContent = dll;
      formMsg("");
      renderUpscalers();
      renderFrameGen();
      renderSettings();
    } catch (e) {
      formMsg(`<div class="cartridge-warn">${esc(String(e))}</div>`);
    }
  };

  $('[data-pc="cancel"]').onclick = () => renderIdle(ctx, el);
  const install = $('[data-pc="install"]');
  install.onclick = async () => {
    const custom = el.querySelector('input[name="pcSource"][value="custom"]').checked;
    if (custom && !form.custom) {
      formMsg(`<div class="cartridge-warn">Elegí el OptiScaler.dll de tu build.</div>`);
      return;
    }
    const anticheat = $('[data-pc="anticheat"]');
    if (anticheat && !anticheat.checked) {
      formMsg(`<div class="cartridge-warn">Confirmá que entendés el riesgo del anti-cheat.</div>`);
      return;
    }
    const upscalers = {};
    el.querySelectorAll("[data-key]").forEach(s => (upscalers[s.dataset.key] = s.value));
    const dlssInputs = $('[data-pc="dlss-inputs"]');
    const fgOn = $('[data-pc="fg-on"]');
    const request = {
      proxy: $('[data-pc="proxy"]').value,
      upscalers,
      dlss_inputs: dlssInputs ? dlssInputs.checked : true,
      nvidia: d.nvidia,
      custom_dll: custom ? form.custom : null,
      frame_gen:
        fgOn && fgOn.checked ? { input: $('[data-pc="fg-input"]').value, output: $('[data-pc="fg-output"]').value } : null,
      fsr_pattern: $('[data-pc="fsr-pattern"]').checked,
      settings: readSettings($('[data-pc="settings-box"]')),
      accept_anticheat: !!anticheat,
    };
    // The form stays as the user left it: an error shows under it.
    install.disabled = true;
    formMsg(`<div class="modal-hint">Copiando OptiScaler y respaldando los archivos del juego que reemplaza...</div>`);
    try {
      showInstalled(ctx, await invoke("optiscaler_install", { source: ctx.source, id: ctx.id, request }));
    } catch (e) {
      install.disabled = false;
      formMsg(`<div class="cartridge-warn">${esc(String(e))}</div>`);
      // An install that failed and couldn't roll itself back is recorded:
      // the tab has to show it (and its Revertir).
      const now = await invoke("pc_detect", { source: ctx.source, id: ctx.id }).catch(() => null);
      if (now && now.optiscaler && el.isConnected) ctx.refresh(`<div class="cartridge-warn">${esc(String(e))}</div>`);
    }
  };
}

function renderInstalled(ctx, el) {
  ctx.editing = null;
  const d = ctx.d;
  const m = d.optiscaler;
  const where = `como <b>${esc(m.meta.proxy || "")}</b>${m.meta.target ? ` en ${esc(m.meta.target)}` : ""}`;
  const head =
    m.state === "applying"
      ? `<div class="cartridge-warn">La instalación de OptiScaler ${esc(m.meta.version || "")} ${where} se cortó a la ` +
        `mitad. "Revertir" deshace lo que llegó a copiar.</div>`
      : m.state === "conflict"
        ? `<div class="cartridge-warn">El último "Revertir" no pudo terminar: estos archivos cambiaron después del ` +
          `parche (¿una actualización del juego?) y quedaron como los dejó el juego. El original de cada uno sigue ` +
          `respaldado.${fileList(m.backed_up)}Si querés los originales, borrá esos archivos y tocá "Revertir" otra ` +
          `vez; si no, "Conservar los del juego" descarta los respaldos.</div>`
        : `<div class="import-result import-result-ok">✓ OptiScaler ${esc(m.meta.version || "")} instalado ${where}.</div>`;
  el.innerHTML =
    head +
    (m.meta.frame_gen ? `<div class="modal-hint">${esc(frameGenText(m.meta.frame_gen))}</div>` : "") +
    (m.meta.fsr_pattern ? `<div class="modal-hint">Busca FSR 2/3 dentro del ejecutable.</div>` : "") +
    (m.state === "applied" ? backedUpHtml(m.backed_up) : "") +
    launchStatusHtml(d) +
    (m.state === "applied"
      ? `<div class="modal-hint">En el juego: activá DLSS, FSR o XeSS en sus opciones gráficas y apretá <b>INSERT</b> para el menú de OptiScaler.</div>`
      : "") +
    `<div class="cartridge-actions">` +
    (m.state === "applied" ? `<button class="cartridge-btn-secondary" data-pc="settings">⚙ Ajustes de imagen</button>` : "") +
    `<button class="cartridge-btn-secondary" data-pc="revert">↺ Revertir parche</button>` +
    (m.state === "conflict"
      ? `<button class="cartridge-btn-secondary" data-pc="keep">Conservar los del juego</button>`
      : "") +
    `</div><div data-pc="settings-edit"></div><div data-pc="revert-msg" aria-live="polite"></div>`;
  installLaunchButton(ctx, el);
  const settingsButton = el.querySelector('[data-pc="settings"]');
  if (settingsButton) settingsButton.onclick = () => editSettings(ctx, el, settingsButton);
  const revertMsg = el.querySelector('[data-pc="revert-msg"]');
  el.querySelector('[data-pc="revert"]').onclick = async e => {
    if (!confirm("¿Quitar OptiScaler y devolver los archivos originales del juego?")) return;
    e.target.disabled = true;
    e.target.textContent = "Revirtiendo...";
    try {
      showReverted(ctx, await invoke("optiscaler_revert", { source: ctx.source, id: ctx.id }));
    } catch (err) {
      e.target.disabled = false;
      e.target.textContent = "↺ Revertir parche";
      revertMsg.innerHTML = `<div class="cartridge-warn">${esc(String(err))}</div>`;
    }
  };
  const keep = el.querySelector('[data-pc="keep"]');
  if (keep)
    keep.onclick = async () => {
      if (!confirm("¿Dejar los archivos como están ahora y borrar los respaldos de los originales?")) return;
      keep.disabled = true;
      try {
        const launch = await invoke("pc_patch_keep_current", { source: ctx.source, id: ctx.id, patchId: m.id });
        const line = launch.status !== "not_needed" ? `<div class="cartridge-guide">${launchText(launch, d)}</div>` : "";
        ctx.refresh(`<div class="import-result import-result-ok">✓ Listo: quedaron los archivos del juego.</div>${line}`);
      } catch (err) {
        keep.disabled = false;
        revertMsg.innerHTML = `<div class="cartridge-warn">${esc(String(err))}</div>`;
      }
    };
}

// The installed OptiScaler's image settings, read from its ini (the in-game
// menu may have changed them) and saved back into it.
async function editSettings(ctx, el, button) {
  const box = el.querySelector('[data-pc="settings-edit"]');
  button.disabled = true;
  ctx.editing = { installed: true };
  let settings;
  try {
    const current = await invoke("optiscaler_settings", { source: ctx.source, id: ctx.id });
    settings = current.settings;
    const choices = current.choices;
    box.innerHTML =
      `<div class="pc-form">${settingsHtml(settings, { nvidia: ctx.d.nvidia, fsr4: !!fsr4Note(ctx.d.gpus), choices })}</div>` +
      `<div class="cartridge-actions"><button class="cartridge-btn" data-pc="settings-save">Guardar</button>` +
      `<button class="cartridge-btn-secondary" data-pc="settings-close">Cerrar</button></div>` +
      `<div data-pc="settings-msg" aria-live="polite"></div>`;
    installSettings(box);
  } catch (e) {
    ctx.editing = null;
    button.disabled = false;
    box.innerHTML = `<div class="cartridge-warn">${esc(String(e))}</div>`;
    return;
  }
  const msg = box.querySelector('[data-pc="settings-msg"]');
  const save = box.querySelector('[data-pc="settings-save"]');
  box.querySelector('[data-pc="settings-close"]').onclick = () => {
    ctx.editing = null;
    box.innerHTML = "";
    button.disabled = false;
    button.focus();
  };
  // "✓ Guardado" is about what was saved, not what's being changed now.
  box.addEventListener("change", () => (msg.innerHTML = ""));
  save.onclick = async () => {
    save.disabled = true;
    try {
      await invoke("optiscaler_save_settings", { source: ctx.source, id: ctx.id, settings: readSettings(box, settings) });
      msg.innerHTML = `<div class="import-result import-result-ok">✓ Guardado: se aplica la próxima vez que abras el juego.</div>`;
    } catch (e) {
      msg.innerHTML = `<div class="cartridge-warn">${esc(String(e))}</div>`;
    } finally {
      save.disabled = false;
    }
  };
}

function frameGenText(fg) {
  const [input, output] = fg.split(" → ");
  return `Frame generation: toma los cuadros ${FG_INPUT_SHORT[input] || `de ${input}`} y los genera con ${FG_OUTPUT_SHORT[output] || output}.`;
}

function fileList(files) {
  return files.length ? `<ul class="pc-list">${files.map(f => `<li>${esc(f)}</li>`).join("")}</ul>` : "";
}

function backedUpHtml(files) {
  return files.length
    ? `<div class="cartridge-guide">Archivos del juego que reemplazó (respaldados; vuelven al revertir):${fileList(files)}</div>`
    : `<div class="modal-hint">No reemplazó ningún archivo del juego: solo agregó los suyos.</div>`;
}

// On Linux, when the proxy name needs WINEDLLOVERRIDES: what the Steam launch
// options have right now (whoever wrote it), or the reminder for another
// launcher.
function launchStatusHtml(d) {
  if (!d.linux) return "";
  const proxy = d.optiscaler.meta.proxy || "";
  if (PROTON_NATIVE.includes(proxy)) return "";
  const stem = proxy.replace(/\.dll$/i, "");
  if (!d.linux.steam) {
    return (
      `<div class="cartridge-guide">Recordá tener <b>WINEDLLOVERRIDES="${esc(stem)}=n,b"</b> en las variables de ` +
      `entorno del lanzador del juego: sin eso Proton no carga OptiScaler.</div>`
    );
  }
  const mode = d.linux.proxy_override;
  if (mode && mode.trim().toLowerCase().startsWith("n")) {
    return `<div class="modal-hint">✓ Las opciones de lanzamiento de Steam cargan ${esc(stem)} (${esc(mode)}).</div>`;
  }
  if (mode) {
    return (
      `<div class="cartridge-warn">Tus opciones de lanzamiento de Steam tienen <b>${esc(stem)}=${esc(mode)}</b>: ` +
      `OptiScaler no carga hasta que lo cambies a ${esc(stem)}=n,b.</div>`
    );
  }
  return (
    `<div class="cartridge-warn">Falta WINEDLLOVERRIDES="${esc(stem)}=n,b" en las opciones de lanzamiento de Steam: sin eso Proton no carga OptiScaler.</div>` +
    `<div class="cartridge-actions"><button class="cartridge-btn-secondary" data-pc="launch-sync">Cerrar Steam y aplicar</button></div>` +
    `<div data-pc="launch-msg" aria-live="polite"></div>`
  );
}

// An override Tatu added for an earlier install and couldn't take back
// (Steam was open during the revert). Harmless without the DLL, but ours.
function leftoverLaunchHtml(d) {
  const change = d.linux && d.linux.launch_change;
  if (!change) return "";
  return (
    `<div class="cartridge-guide">Quedó <b>WINEDLLOVERRIDES=${esc(change.dll)}=n,b</b> en las opciones de lanzamiento de ` +
    `una instalación anterior (no hace nada sin OptiScaler).</div>` +
    `<div class="cartridge-actions"><button class="cartridge-btn-secondary" data-pc="launch-sync">Cerrar Steam y quitarla</button></div>` +
    `<div data-pc="launch-msg" aria-live="polite"></div>`
  );
}

function installLaunchButton(ctx, el) {
  const btn = el.querySelector('[data-pc="launch-sync"]');
  if (!btn) return;
  const label = btn.textContent;
  btn.onclick = async () => {
    btn.disabled = true;
    btn.textContent = "Cerrando Steam...";
    try {
      const report = await invoke("pc_launch_sync", { source: ctx.source, id: ctx.id, closeSteam: true });
      ctx.refresh(`<div class="cartridge-guide">${launchText(report, ctx.d)}</div>`);
    } catch (e) {
      btn.disabled = false;
      btn.textContent = label;
      el.querySelector('[data-pc="launch-msg"]').innerHTML = `<div class="cartridge-warn">${esc(String(e))}</div>`;
    }
  };
}

function launchText(report, d) {
  const detail = esc(report.detail || "");
  switch (report.status) {
    case "applied":
      return `Agregué WINEDLLOVERRIDES con <b>${detail}</b> a las opciones de lanzamiento de Steam.`;
    case "removed":
      return `Quité <b>${detail}</b> de las opciones de lanzamiento de Steam.`;
    case "already":
      return `Las opciones de lanzamiento de Steam ya cargan <b>${detail}</b>.`;
    case "steam_running":
      return `Falta ajustar <b>${detail}</b> en las opciones de lanzamiento: Steam está abierto. Hacelo desde la pestaña PC con "Cerrar Steam y aplicar".`;
    case "conflict":
    case "manual":
      return detail;
    case "error":
      return `No pude editar las opciones de lanzamiento: ${detail}`;
    default:
      return d && d.linux
        ? "En Linux no hace falta tocar las opciones de lanzamiento: Proton ya carga OptiScaler desde la carpeta del juego."
        : "";
  }
}

function showInstalled(ctx, result) {
  ctx.editing = null;
  // The window moved on to another game: its tab shows the install next time.
  if (!ctx.el.isConnected) return;
  const m = result.patch;
  const launch = launchText(result.launch, ctx.d);
  popup(
    "OptiScaler instalado",
    `<div class="import-result import-result-ok">✓ OptiScaler ${esc(m.meta.version || "")} quedó instalado como <b>${esc(m.meta.proxy || "")}</b>.</div>` +
      backedUpHtml(m.backed_up) +
      (launch ? `<div class="cartridge-guide">${launch}</div>` : "") +
      `<div class="cartridge-guide"><b>Para usarlo</b><ol>` +
      `<li>Abrí el juego y activá el upscaler que traiga (DLSS, FSR o XeSS) en sus opciones gráficas.</li>` +
      `<li>Apretá <b>INSERT</b>: aparece el menú de OptiScaler. Ahí elegís el upscaler que se usa en su lugar y se guarda para la próxima.</li>` +
      (m.meta.frame_gen
        ? `<li>Frame generation: si lo toma del juego, activá su opción de frame generation (por ejemplo DLSS Frame Generation). En el menú de OptiScaler podés cambiarlo o apagarlo.</li>`
        : "") +
      `<li>Para sacarlo: pestaña PC → <b>Revertir parche</b>.</li></ol></div>`,
  );
  ctx.refresh();
}

function showReverted(ctx, { report, launch }) {
  if (!ctx.el.isConnected) return;
  const done = !report.conflicts.length && !report.errors.length;
  const list = (title, files) => (files.length ? `<div class="cartridge-guide">${title}${fileList(files)}</div>` : "");
  const launchLine = launch && launch.status !== "not_needed" ? `<div class="cartridge-guide">${launchText(launch, ctx.d)}</div>` : "";
  popup(
    "OptiScaler",
    (done
      ? `<div class="import-result import-result-ok">✓ Parche revertido: el juego quedó como estaba.</div>`
      : `<div class="cartridge-warn">Revertido en parte.</div>`) +
      list("Archivos originales del juego restaurados:", report.restored) +
      (report.removed.length ? `<div class="modal-hint">Quité ${report.removed.length} archivos de OptiScaler.</div>` : "") +
      list("Estos los cambió otra cosa después del parche y los dejé como están:", report.kept) +
      list(
        "Estos cambiaron después del parche (¿una actualización del juego?): quedaron como están y el original sigue respaldado. Si querés el original, borralos y revertí otra vez:",
        report.conflicts,
      ) +
      list("Errores (cerrá el juego y probá revertir otra vez):", report.errors) +
      launchLine,
  );
  ctx.refresh();
}

function popup(title, html) {
  const overlay = document.getElementById("pcOverlay");
  document.getElementById("pcOverlayTitle").textContent = title;
  document.getElementById("pcOverlayBody").innerHTML =
    html + `<div class="cartridge-actions"><button class="cartridge-btn" id="pcOverlayOk">Entendido</button></div>`;
  const close = () => overlay.classList.add("hidden");
  document.getElementById("pcOverlayOk").onclick = close;
  document.getElementById("pcOverlayClose").onclick = close;
  overlay.onclick = e => {
    if (e.target === overlay) close();
  };
  overlay.classList.remove("hidden");
}
