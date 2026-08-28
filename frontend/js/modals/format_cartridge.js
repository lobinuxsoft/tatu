import { invoke } from "../tauri.js";
import { esc, formatBytes } from "../utils.js";

// Shared by both the per-game "Instalar en un cartucho" flow (cartridge.js)
// and the Cartucho tab's own standalone "Formatear/Reformatear" entry point
// (cartridge_manage.js, #232) — one destructive confirmation, one call to
// `format_as_cartridge`, each caller decides what happens before/after.
export function renderFormatConfirm(el, drive, { onCancel, onSuccess, onError }) {
  el.innerHTML =
    `<div class="cartridge-warn">Vas a formatear "<b>${esc(drive.label || drive.id)}</b>" ` +
    `(${formatBytes(drive.total_bytes)}) como cartucho. Esto BORRA todo su contenido actual, sin vuelta atrás.</div>` +
    `<div class="cartridge-actions">` +
    `<button class="cartridge-btn-secondary" id="cartFormatCancel">Volver</button>` +
    `<button class="cartridge-btn" id="cartFormatGo">Formatear</button>` +
    `</div>`;

  document.getElementById("cartFormatCancel").onclick = onCancel;
  document.getElementById("cartFormatGo").onclick = async () => {
    el.innerHTML = `<div class="loading"><div class="spinner"></div><br>Formateando...</div>`;
    try {
      await invoke("format_as_cartridge", {
        device: drive.id,
        expectedLabel: drive.label,
        expectedBytes: drive.total_bytes,
      });
      const fresh = (await invoke("list_removable_drives")).find(d => d.id === drive.id);
      if (!fresh || !fresh.mount_point) {
        el.innerHTML = `<div class="cartridge-warn">Formateado, pero no encontré el punto de montaje. Reconectá el disco e intentá de nuevo.</div>`;
        return;
      }
      onSuccess(fresh);
    } catch (e) {
      // format_as_cartridge isn't registered at all on Windows yet (#194) —
      // Tauri's own "command not found" error is how that surfaces here.
      const raw = String(e);
      const msg = /not found|unknown command/i.test(raw)
        ? "Formatear todavía no está soportado en Windows (ver #194)."
        : raw;
      onError(msg);
    }
  };
}
