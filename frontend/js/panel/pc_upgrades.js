import { invoke, listen } from "../tauri.js";
import { esc, formatBytes, downloadToken } from "../utils.js";
import { fsr4Note } from "./pc_optiscaler.js";

// Newer DLSS / FSR / XeSS without OptiScaler (PC tab). Swapping a game's DLL,
// as a reversible patch, for its vendor's newest official one — downloaded
// by the user's Tatu from NVIDIA's, AMD's or Intel's own SDK repository,
// never hosted by Tatu — or, for DLSS, a copy the user already has; on
// Linux, GE-Proton's own switches in the Steam launch options (it downloads
// the DLLs itself, into the prefix — no game file changes). Valve's Proton
// has no equivalent.

const UPGRADES = [
  ["PROTON_DLSS_UPGRADE", "DLSS más nuevo", "Descarga la última versión de DLSS de NVIDIA y usa su preset más reciente."],
  ["PROTON_FSR4_UPGRADE", "FSR 3.1 → FSR 4", "Los juegos con FSR 3.1 usan FSR 4. Placas AMD RDNA4."],
  ["PROTON_FSR4_RDNA3_UPGRADE", "FSR 3.1 → FSR 4 en RDNA3", "Lo mismo para placas AMD RDNA3."],
];

export function upgradesSectionHtml() {
  return `<div class="detail-info-row"><span class="detail-info-label">DLSS, FSR y XeSS más nuevos</span></div><div data-pc="upgrades"></div>`;
}

export function installUpgradesSection(ctx) {
  const d = ctx.d;
  const el = ctx.el.querySelector('[data-pc="upgrades"]');
  const swaps = d.patches.filter(p => p.meta && ["dlss", "fsr", "xess"].includes(p.meta.kind));
  const update = u => d.official_updates.find(x => x.rel === u.rel);
  // A game DLL gets a row when there's an official update for it, or — DLSS,
  // where DLSS runs — to swap in one the user has.
  const ownDlss = u => d.nvidia && u.kind.startsWith("dlss");
  const rows = d.scan.upscalers.filter(u => !u.patched_by && (update(u) || ownDlss(u)));
  const updates = rows.map(update).filter(Boolean);

  // Values from disk (patch ids, file names) go to the handlers by index,
  // never through an HTML attribute.
  let html =
    swaps.map((p, i) => swapHtml(p, i)).join("") +
    rows
      .map((u, i) => {
        const up = update(u);
        return (
          `<div class="pc-found"><b>${esc(u.label)}</b> ${esc(u.version || "")} ` +
          (up ? `<button class="cartridge-btn" data-official="${i}">⬇ Actualizar a ${esc(up.release)}</button>` : "") +
          (ownDlss(u) ? `<button class="cartridge-btn-secondary" data-swap="${i}">Usar uno propio…</button>` : "") +
          `<div class="pc-path">${esc(u.rel)}</div></div>`
        );
      })
      .join("");
  if (updates.length) {
    const vendors = [
      updates.some(x => x.sdk === "dlss") && "NVIDIA (github.com/NVIDIA/DLSS; se usa bajo su licencia, solo en placas NVIDIA)",
      updates.some(x => x.sdk === "fsr") && "AMD (su FidelityFX SDK)",
      updates.some(x => x.sdk === "xess") && "Intel (su XeSS SDK)",
    ].filter(Boolean);
    html +=
      `<div class="modal-hint">«Actualizar» baja la versión oficial más reciente del repositorio de ${vendors.join(", ")}, ` +
      `verificada: Tatu no aloja ningún DLL. El del juego queda respaldado y «Revertir» lo devuelve si el juego se ` +
      `lleva mal con la versión nueva.</div>`;
  }
  if (rows.some(ownDlss)) {
    html +=
      `<div class="modal-hint">«Usar uno propio» toma un ${esc(fileName(rows.find(ownDlss).rel))} que ya tengas, por ` +
      `ejemplo de otro juego.</div>`;
  }
  // FSR 2 and Vulkan FSR have no DLL a newer version drops into.
  if (d.scan.upscalers.some(u => u.kind === "fsr2" || /_vk\.dll$/i.test(u.rel))) {
    html +=
      `<div class="modal-hint">El FSR 2 y el FSR de Vulkan no se actualizan cambiando el DLL: para usar otro upscaler ` +
      `en su lugar está OptiScaler.</div>`;
  }

  if (d.linux && d.linux.steam) {
    html += protonHtml(d);
  } else if (d.linux) {
    const vars = [
      d.nvidia && `<b>PROTON_DLSS_UPGRADE=1</b> (DLSS más nuevo)`,
      fsr4Note(d.gpus) && `<b>PROTON_FSR4_UPGRADE=1</b> (FSR 3.1 → FSR 4)`,
      (d.gpus[0] || {}).vendor === "amd" &&
        (d.gpus[0].rdna === 3 || !d.gpus[0].rdna) &&
        `<b>PROTON_FSR4_RDNA3_UPGRADE=1</b> (FSR 4 en RDNA 3)`,
    ].filter(Boolean);
    if (vars.length)
      html += `<div class="modal-hint">Con GE-Proton podés agregar ${vars.join(" o ")} a las variables de entorno del lanzador del juego.</div>`;
  } else if (rows.some(ownDlss)) {
    html +=
      `<div class="modal-hint">Sin tocar archivos: NVIDIA App → Gráficos → este juego → <b>Anulación de DLSS</b>, ` +
      `si el juego lo admite (el driver usa el modelo más nuevo).</div>`;
  }
  el.innerHTML =
    (html ||
      (d.scan.upscalers.some(u => !u.patched_by)
        ? `<div class="modal-hint">No hay versiones oficiales más nuevas para los DLL de este juego${
            d.scan.upscalers.some(u => u.kind.startsWith("dlss")) && !d.nvidia ? " (DLSS solo se actualiza en placas NVIDIA RTX)" : ""
          }.</div>`
        : `<div class="modal-hint">No encontré DLLs de DLSS, FSR ni XeSS en este juego.</div>`)) +
    `<div data-pc="upgrade-msg" aria-live="polite"></div>`;

  const msg = el.querySelector('[data-pc="upgrade-msg"]');
  const fail = e => (msg.innerHTML = `<div class="cartridge-warn">${esc(String(e))}</div>`);
  if (ctx.update) showUpdate(ctx);

  el.querySelectorAll("[data-swap]").forEach(btn => {
    const u = rows[Number(btn.dataset.swap)];
    btn.onclick = async () => {
      const ac = d.scan.anti_cheat;
      if (
        ac.length &&
        !confirm(`Este juego trae anti-cheat (${ac.join(", ")}). Cambiarle DLLs puede terminar en un ban si jugás online. ¿Seguir?`)
      )
        return;
      btn.disabled = true;
      const file = await invoke("pc_pick_dll", { startIn: null }).catch(() => null);
      if (!file) {
        btn.disabled = false;
        return;
      }
      try {
        await invoke("dlss_swap", { source: ctx.source, id: ctx.id, rel: u.rel, file, acceptAnticheat: ac.length > 0 });
        ctx.refresh();
      } catch (e) {
        btn.disabled = false;
        fail(e);
      }
    };
  });
  el.querySelectorAll("[data-official]").forEach(btn => {
    const u = rows[Number(btn.dataset.official)];
    const up = update(u);
    btn.onclick = async () => {
      const ac = d.scan.anti_cheat;
      if (
        ac.length &&
        !confirm(`Este juego trae anti-cheat (${ac.join(", ")}). Cambiarle DLLs puede terminar en un ban si jugás online. ¿Seguir?`)
      )
        return;
      ctx.update = { text: `Preparando ${up.release}...`, cancelling: false, token: downloadToken() };
      showUpdate(ctx);
      const unlisten = await listen("runtime_progress", e => {
        const { done, total } = e.payload || {};
        if (!ctx.update) return;
        ctx.update.text = `Descargando ${up.release}: ${formatBytes(done)} / ${formatBytes(total)}`;
        const line = ctx.el.querySelector('[data-pc="dlss-progress"]');
        if (line) line.textContent = ctx.update.text;
      });
      // The tab may have been redrawn meanwhile: the outcome goes on top of
      // a fresh one either way.
      let note;
      try {
        await invoke("runtime_update", {
          source: ctx.source,
          id: ctx.id,
          rel: u.rel,
          acceptAnticheat: ac.length > 0,
          cancelToken: ctx.update.token,
        });
        note = `<div class="import-result import-result-ok">✓ ${esc(fileName(u.rel))} actualizado a ${esc(up.release)} (${esc(up.version)}).</div>`;
      } catch (e) {
        note = `<div class="cartridge-warn">${esc(String(e))}</div>`;
      } finally {
        unlisten();
        ctx.update = null;
      }
      ctx.refresh(note);
    };
  });
  el.querySelectorAll("[data-revert]").forEach(btn => {
    const p = swaps[Number(btn.dataset.revert)];
    btn.onclick = async () => {
      btn.disabled = true;
      try {
        const report = await invoke("pc_patch_revert", { source: ctx.source, id: ctx.id, patchId: p.id });
        const left = [...report.conflicts.map(f => `${f} cambió después del parche; quedó como está.`), ...report.errors];
        ctx.refresh(left.length ? `<div class="cartridge-warn">${esc(left.join(" "))}</div>` : "");
      } catch (e) {
        btn.disabled = false;
        fail(e);
      }
    };
  });
  el.querySelectorAll("[data-keep]").forEach(btn => {
    const p = swaps[Number(btn.dataset.keep)];
    btn.onclick = async () => {
      if (!confirm("¿Dejar el DLL que tiene ahora el juego y borrar el respaldo del original?")) return;
      btn.disabled = true;
      try {
        await invoke("pc_patch_keep_current", { source: ctx.source, id: ctx.id, patchId: p.id });
        ctx.refresh();
      } catch (e) {
        btn.disabled = false;
        fail(e);
      }
    };
  });
  el.querySelectorAll("[data-upgrade]").forEach(box => {
    box.onchange = () => setUpgrade(ctx, box, msg, false);
  });
}

// The update in flight (`ctx.update`), in whatever the tab shows now: its
// progress and Cancel, and the other DLL buttons off until it ends.
function showUpdate(ctx) {
  const box = ctx.el.querySelector('[data-pc="upgrade-msg"]');
  if (!box) return;
  ctx.el.querySelectorAll("[data-official], [data-swap]").forEach(b => (b.disabled = true));
  box.innerHTML =
    `<div class="modal-hint" data-pc="dlss-progress">${esc(ctx.update.text)}</div>` +
    `<div class="cartridge-actions"><button class="cartridge-btn-secondary" data-pc="dlss-cancel"` +
    `${ctx.update.cancelling ? " disabled" : ""}>Cancelar</button></div>`;
  const token = ctx.update.token;
  box.querySelector('[data-pc="dlss-cancel"]').onclick = e => {
    e.target.disabled = true;
    ctx.update.cancelling = true;
    invoke("pc_cancel_download", { token }).catch(() => {});
  };
}

function swapHtml(p, i) {
  const name = esc(fileName(p.meta.file));
  const note =
    p.state === "applying"
      ? `<div class="cartridge-warn">El reemplazo se cortó a la mitad: "Revertir" lo deshace.</div>`
      : p.state === "conflict"
        ? `<div class="cartridge-warn">${name} cambió después del parche (¿una actualización del juego?) y quedó como lo ` +
          `dejó el juego; el original sigue respaldado. Si querés el original, borralo y tocá "Revertir"; si no, ` +
          `"Conservar el del juego".</div>`
        : "";
  return (
    `<div class="pc-found"><b>${name}</b> reemplazado: ${esc(p.meta.from_version || "?")} → ` +
    `${esc(p.meta.to_version || "?")} <button class="cartridge-btn-secondary" data-revert="${i}">↺ Revertir</button>` +
    (p.state === "conflict" ? ` <button class="cartridge-btn-secondary" data-keep="${i}">Conservar el del juego</button>` : "") +
    `<div class="pc-path">${esc(p.meta.file)}</div>${note}</div>`
  );
}

// Each switch only for the GPU it's for: DLSS on NVIDIA's driver, FSR 4 on
// RDNA 4 / RDNA 3 — both when an AMD card's generation is unknown. A switch
// already on always shows, to turn it off.
function forThisGpu(key, d) {
  if (key === "PROTON_DLSS_UPGRADE") return d.nvidia;
  // The GPU games run on (listed first), not an iGPU next to it.
  const g = d.gpus[0];
  if (!g || g.vendor !== "amd") return false;
  return !g.rdna || (key === "PROTON_FSR4_UPGRADE" ? g.rdna >= 4 : g.rdna === 3);
}

function protonHtml(d) {
  const linux = d.linux;
  if (linux.launch_options_error) {
    return `<div class="cartridge-warn">No pude leer las opciones de lanzamiento de Steam: ${esc(linux.launch_options_error)}</div>`;
  }
  const switches = UPGRADES.filter(([key]) => linux.upgrades[key] || forThisGpu(key, d));
  if (!switches.length) return "";
  const tool = linux.compat_tool || "la predeterminada de Steam";
  return (
    `<div class="pc-proton">` +
    `<div class="modal-hint">Opciones de GE-Proton (se agregan a las opciones de lanzamiento de Steam). Herramienta de ` +
    `compatibilidad de este juego: <b>${esc(tool)}</b>.</div>` +
    (linux.ge_proton
      ? ""
      : `<div class="cartridge-warn">Estas opciones son de GE-Proton: con otro Proton no hacen nada. Elegí GE-Proton en ` +
        `Steam → Propiedades del juego → Compatibilidad.</div>`) +
    switches.map(
      ([key, label, hint]) =>
        `<label class="pc-check"><input type="checkbox" data-upgrade="${key}"${linux.upgrades[key] ? " checked" : ""}> ` +
        `<b>${esc(label)}</b> <span class="pc-path">${esc(hint)}</span></label>`,
    ).join("") +
    `</div>`
  );
}

// The box only ever shows what's written: a change Steam blocked goes back,
// and the retry puts it through.
async function setUpgrade(ctx, box, msg, closeSteam) {
  const key = box.dataset.upgrade;
  const on = box.checked;
  box.disabled = true;
  msg.innerHTML = closeSteam ? `<div class="modal-hint">Cerrando Steam...</div>` : "";
  try {
    const report = await invoke("pc_set_proton_upgrade", { source: ctx.source, id: ctx.id, key, on, closeSteam });
    if (report.status === "applied") {
      ctx.d.linux.upgrades[key] = on;
      msg.innerHTML = "";
      return;
    }
    box.checked = !on;
    if (report.status !== "steam_running") {
      msg.innerHTML = `<div class="cartridge-warn">${esc(report.detail)}</div>`;
      return;
    }
    // Steam rewrites the file on exit: it has to be closed first.
    msg.innerHTML =
      `<div class="cartridge-warn">Steam está abierto y pisaría el cambio al cerrarse.</div>` +
      `<div class="cartridge-actions"><button class="cartridge-btn-secondary" data-pc="close-steam">Cerrar Steam y aplicar</button></div>`;
    const retry = msg.querySelector('[data-pc="close-steam"]');
    retry.onclick = () => {
      retry.disabled = true;
      box.checked = on;
      setUpgrade(ctx, box, msg, true);
    };
  } catch (e) {
    box.checked = !on;
    msg.innerHTML = `<div class="cartridge-warn">${esc(String(e))}</div>`;
  } finally {
    box.disabled = false;
  }
}

function fileName(rel) {
  return (rel || "").split("/").pop();
}
