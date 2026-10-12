import { invoke } from "../tauri.js";
import { esc } from "../utils.js";
import { loadingPlaceholder } from "./detail_template.js";
import { optiscalerSectionHtml, installOptiscalerSection } from "./pc_optiscaler.js";
import { upgradesSectionHtml, installUpgradesSection } from "./pc_upgrades.js";

// "PC" tab — right-aligned next to GameNative: what this installed copy of
// the game can do on this PC. The upscalers it ships (with versions),
// OptiScaler to use FSR / XeSS / DLSS in place of the one it has, newer DLSS.
// Every change is a reversible patch: the game's own files it replaces are
// backed up inside the game folder (`.tatu-patches`) and "Revertir" puts them
// back. One module per section, like the GameNative tab.
//
// Everything renders inside `ctx.el`, never by global id: the detail window
// is reused for another game, and a slow scan of the previous one must land
// in its own (detached) tab, not in the new game's.

export function pcTabHtml() {
  return `<div id="pcTabBody">${loadingPlaceholder("Revisando la carpeta del juego...")}</div>`;
}

// `source`: steam · non_steam · gog · egs. The folder is only read once the
// tab is opened — it walks the whole game.
export function installPcTab(source, id) {
  const el = document.getElementById("pcTabBody");
  const tab = document.querySelector('.detail-tab[data-dp="pc"]');
  if (!el || !tab) return;
  const ctx = { source, id, el, d: null, loading: false, gen: 0 };
  ctx.refresh = note => refresh(ctx, note);
  tab.addEventListener("click", () => {
    if (!ctx.d && !ctx.loading) ctx.refresh();
  });
}

// `note`: HTML shown above the summary (the outcome of what triggered it).
async function refresh(ctx, note = "") {
  // Only the latest refresh renders: an older, slower one never overwrites it.
  const gen = ++ctx.gen;
  ctx.loading = true;
  // What the user is in the middle of in the OptiScaler section (the install
  // form, a download, the settings editor) survives: the node is kept and
  // put back when OptiScaler is still (not) installed, as it was.
  const editing = ctx.editing;
  const kept = editing && ctx.el.querySelector('[data-pc="opti"]');
  if (kept) kept.remove();
  ctx.el.innerHTML = loadingPlaceholder("Revisando la carpeta del juego...");
  try {
    const d = await invoke("pc_detect", { source: ctx.source, id: ctx.id });
    if (gen !== ctx.gen) return;
    ctx.d = d;
    ctx.el.innerHTML = note + summaryHtml(d) + optiscalerSectionHtml() + upgradesSectionHtml();
    installSummary(ctx);
    installOptiscalerSection(ctx);
    if (kept && editing.installed === !!d.optiscaler) {
      ctx.el.querySelector('[data-pc="opti"]').replaceWith(kept);
      ctx.editing = editing;
    }
    installUpgradesSection(ctx);
  } catch (e) {
    if (gen !== ctx.gen) return;
    ctx.d = null;
    ctx.el.innerHTML = `<div class="cartridge-warn">${esc(String(e))}</div>`;
  } finally {
    if (gen === ctx.gen) ctx.loading = false;
  }
}

function summaryHtml(d) {
  const upscalers = d.scan.upscalers.length
    ? d.scan.upscalers
        .map(
          u =>
            `<div class="pc-found"><b>${esc(u.label)}</b> ${esc(u.version || "versión desconocida")}` +
            (u.patched_by ? ` <span class="pc-path">(puesto por un parche de Tatu)</span>` : "") +
            `<div class="pc-path">${esc(u.rel)}</div></div>`,
        )
        .join("")
    : `<span class="detail-info-value">No encontré DLLs de DLSS, FSR ni XeSS. Puede traer FSR 2 o 3 compilado ` +
      `dentro del ejecutable: OptiScaler puede buscarlo ahí.</span>`;

  return (
    `<p class="modal-hint">Lo que tiene esta copia del juego instalada en la PC. Todo lo que cambies desde acá ` +
    `respalda los archivos originales del juego y se revierte.</p>` +
    (!d.exe && d.linux && !d.target_custom
      ? `<div class="cartridge-warn">No encontré un ejecutable de Windows. Si es la versión nativa de Linux, nada de ` +
        `esto aplica (funciona solo con Proton); si no, elegí la carpeta del ejecutable con "Cambiar…".</div>`
      : "") +
    (d.scan.anti_cheat.length
      ? `<div class="cartridge-warn">⚠ Anti-cheat: <b>${esc(d.scan.anti_cheat.join(", "))}</b>. Meterle DLLs a un ` +
        `juego online con anti-cheat puede terminar en un ban: usalo solo para jugar sin conexión.</div>`
      : "") +
    (d.broken_patches.length
      ? `<div class="cartridge-warn">No pude leer el registro de ${d.broken_patches.length > 1 ? "estos parches" : "este parche"}: ` +
        `<b>${esc(d.broken_patches.join(", "))}</b>. Tatu no los toca; lo que respaldaron sigue en la carpeta ` +
        `<b>.tatu-patches</b> del juego.</div>`
      : "") +
    `<div class="detail-info-row"><span class="detail-info-label">Upscalers del juego</span>` +
    `<div class="detail-info-value">${upscalers}${d.scan.truncated ? `<div class="pc-path">(el juego tiene demasiados archivos; puede faltar alguno)</div>` : ""}</div></div>` +
    `<div class="detail-info-row"><span class="detail-info-label">GPU</span>` +
    `<span class="detail-info-value">${d.gpus.length ? d.gpus.map(g => esc(gpuText(g))).join("<br>") : "No la pude detectar"}</span></div>` +
    `<div class="detail-info-row"><span class="detail-info-label">Carpeta del ejecutable</span>` +
    `<span class="detail-info-value"><span class="pc-path">${esc(d.target || "(la carpeta principal del juego)")}</span> ` +
    `<button class="cartridge-btn-secondary" data-pc="pick-target">Cambiar…</button>` +
    (d.target_custom ? ` <button class="cartridge-btn-secondary" data-pc="reset-target">Detectar sola</button>` : "") +
    `</span></div><div data-pc="target-msg" aria-live="polite"></div>`
  );
}

function gpuText(g) {
  const vendor = { nvidia: "NVIDIA", amd: "AMD", intel: "Intel" }[g.vendor] || "Otra";
  return (
    (g.name || vendor) +
    (g.rdna ? ` · RDNA ${g.rdna}` : "") +
    // nouveau / NVK, or Windows without NVIDIA's driver.
    (g.vendor === "nvidia" && !g.dlss ? ` (driver ${g.driver || "genérico"}: sin DLSS)` : "")
  );
}

// OptiScaler goes next to the exe that actually runs; the detection covers
// Unreal's Binaries/Win64, this covers everything else.
function installSummary(ctx) {
  const msg = ctx.el.querySelector('[data-pc="target-msg"]');
  const setTarget = async dir => {
    try {
      await invoke("pc_set_target_dir", { source: ctx.source, id: ctx.id, dir });
      ctx.refresh();
    } catch (e) {
      msg.innerHTML = `<div class="cartridge-warn">${esc(String(e))}</div>`;
    }
  };
  const pick = ctx.el.querySelector('[data-pc="pick-target"]');
  pick.onclick = async () => {
    const { root, target } = ctx.d;
    const sep = root.includes("\\") ? "\\" : "/";
    pick.disabled = true;
    const dir = await invoke("pick_non_steam_folder", {
      startIn: target ? root + sep + target.split("/").join(sep) : root,
    }).catch(() => null);
    pick.disabled = false;
    if (dir) setTarget(dir);
  };
  const reset = ctx.el.querySelector('[data-pc="reset-target"]');
  if (reset) reset.onclick = () => setTarget(null);
}
