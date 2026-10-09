import { configSectionHtml, installConfigSection } from "./gamenative_config.js";
import { invoke } from "../tauri.js";
import { sendSectionHtml, installSendSection } from "./gamenative_send.js";

// "GameNative" tab — right-aligned, apart from the game's own data tabs:
// everything about playing this PC-installed game on an Android handheld
// running GameNative (https://github.com/utkarshdalal/GameNative). Only
// stacks sections, one module each, so it can keep growing: the saved best
// configuration first, then sending the game to the device, then measuring
// its performance (its own window: perf.html).

export function gameNativeTabHtml() {
  return (
    `<p class="modal-hint">Jugá este juego instalado en la PC en un teléfono o handheld Android con GameNative.</p>` +
    configSectionHtml() +
    sendSectionHtml() +
    `<div class="detail-info-row"><span class="detail-info-label">Rendimiento</span></div>` +
    `<div class="detail-info-row">` +
    `<button class="cartridge-btn" id="gnPerfBtn">📈 Medir rendimiento</button>` +
    `<span class="detail-info-value">FPS, 1% low, CPU, GPU, RAM, consumo y temperaturas mientras jugás, y comparación entre configuraciones.</span>` +
    `</div>`
  );
}

// `source` is "steam" or "non_steam" — the two sources with a local install.
export function installGameNativeTab(source, id, gameName) {
  installConfigSection(source, id);
  installSendSection(source, id, gameName);
  const perf = document.getElementById("gnPerfBtn");
  if (perf) perf.onclick = () => invoke("open_perf_window", { source, id, name: gameName }).catch(() => {});
}
