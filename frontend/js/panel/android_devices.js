import { invoke } from "../tauri.js";
import { esc } from "../utils.js";
import { loadingPlaceholder } from "./detail_template.js";

// Picking an Android device over adb — shared by "Enviar a Android" and the
// performance window. Renders into `el` and calls `onPick(device)` with a
// ready one. Covers the two ways it goes wrong: no working adb (download it
// or point at one) and no device on a cable (connect over Wi-Fi — also the
// only way power can be measured, since a phone on USB is charging).

const STATE_TAG = {
  device: `<span class="drive-tag drive-tag-ready">Listo</span>`,
  unauthorized: `<span class="drive-tag drive-tag-blank">Aceptá el aviso de depuración en el teléfono</span>`,
  offline: `<span class="drive-tag drive-tag-blank">Desconectado</span>`,
  "no permissions": `<span class="drive-tag drive-tag-blank">Sin permisos (reglas udev de adb)</span>`,
};

const PLATFORM_TOOLS_TERMS = "https://developer.android.com/studio/releases/platform-tools";

// `note`: optional html shown above the list (e.g. why Wi-Fi matters here).
export async function showDevicePicker(el, onPick, { note = "" } = {}) {
  const $ = sel => el.querySelector(sel);
  el.onclick = null;
  el.innerHTML = loadingPlaceholder("Buscando dispositivos...");
  let devices;
  try {
    devices = await invoke("android_list_devices");
  } catch (e) {
    showAdbSetup(el, String(e), () => showDevicePicker(el, onPick, { note }));
    return;
  }

  let html = note;
  if (!devices.length) {
    html += `<div class="cartridge-guide">No hay ningún dispositivo conectado. Conectalo por USB con la <b>depuración USB</b> activada (Opciones de desarrollador), o por Wi-Fi acá abajo, y buscá de nuevo.</div>`;
  }
  for (const d of devices) {
    const tag = STATE_TAG[d.state] || `<span class="drive-tag drive-tag-blank">${esc(d.state)}</span>`;
    const disabled = d.state === "device" ? "" : " disabled";
    // adb names Wi-Fi devices by ip:port.
    const via = d.serial.includes(":") ? " · Wi-Fi" : "";
    html +=
      `<div class="collection-row${disabled}" data-serial="${esc(d.serial)}">` +
      `<span class="collection-name">${esc(d.model || d.serial)}${via}</span>` +
      `<span class="collection-count">${tag}</span>` +
      `</div>`;
  }
  html +=
    `<div class="cartridge-actions"><button class="cartridge-btn-secondary" data-dev="rescan">Buscar de nuevo</button></div>` +
    wifiHtml();
  el.innerHTML = html;

  $('[data-dev="rescan"]').onclick = () => showDevicePicker(el, onPick, { note });
  installWifi(el, () => showDevicePicker(el, onPick, { note }));
  el.onclick = e => {
    const row = e.target.closest(".collection-row");
    if (!row || row.classList.contains("disabled")) return;
    const device = devices.find(d => d.serial === row.dataset.serial);
    if (device) {
      el.onclick = null;
      onPick(device);
    }
  };
}

// Wireless debugging (Android 11+): pair once with the code the phone shows,
// then connect to the address on the Wireless debugging screen itself —
// two different ports.
function wifiHtml() {
  return (
    `<details class="gn-wifi"><summary>Conectar por Wi-Fi</summary>` +
    `<div class="modal-hint">En el teléfono: Opciones de desarrollador → <b>Depuración inalámbrica</b>. ` +
    `La primera vez tocá "Vincular dispositivo con código" y usá esa IP:puerto y ese código; ` +
    `después conectá a la IP:puerto que muestra la pantalla de Depuración inalámbrica.</div>` +
    `<div class="art-search-row">` +
    `<input type="text" data-dev="pairAddr" placeholder="IP:puerto de vinculación">` +
    `<input type="text" data-dev="pairCode" placeholder="Código" maxlength="6" style="max-width:7rem">` +
    `<button class="cartridge-btn-secondary" data-dev="pair">Vincular</button></div>` +
    `<div class="art-search-row">` +
    `<input type="text" data-dev="connAddr" placeholder="IP:puerto de Depuración inalámbrica">` +
    `<button class="cartridge-btn" data-dev="connect">Conectar</button></div>` +
    `<div data-dev="wifiMsg"></div>` +
    `</details>`
  );
}

function installWifi(el, refresh) {
  const $ = sel => el.querySelector(sel);
  const msg = (html, ok) => {
    $('[data-dev="wifiMsg"]').innerHTML = `<div class="${ok ? "import-result import-result-ok" : "cartridge-warn"}">${html}</div>`;
  };
  $('[data-dev="pair"]').onclick = async () => {
    try {
      await invoke("android_pair", { address: $('[data-dev="pairAddr"]').value, code: $('[data-dev="pairCode"]').value });
      msg("Vinculado. Ahora conectá con la IP:puerto de Depuración inalámbrica.", true);
    } catch (e) {
      msg(esc(String(e)), false);
    }
  };
  $('[data-dev="connect"]').onclick = async () => {
    try {
      await invoke("android_connect", { address: $('[data-dev="connAddr"]').value });
      refresh();
    } catch (e) {
      msg(esc(String(e)), false);
    }
  };
}

// Any failure listing devices lands here, not just "adb not found": a wrong
// saved path, or an adb too old to talk to the phone, both get fixed the
// same way — a working adb. Downloading is the easy way: Google's own
// platform-tools, fetched by this machine straight from Google (Tatu can't
// ship them itself — their license forbids redistribution).
function showAdbSetup(el, error, retry) {
  const $ = sel => el.querySelector(sel);
  el.onclick = null;
  el.innerHTML =
    `<div class="cartridge-warn">${esc(error)}</div>` +
    `<div class="cartridge-guide">Tatu puede descargarlo por vos: se baja directo de Google (Android SDK Platform-Tools) ` +
    `y al descargarlo aceptás <a href="${PLATFORM_TOOLS_TERMS}">sus términos</a>.</div>` +
    `<div class="cartridge-actions"><button class="cartridge-btn" data-dev="download">⬇ Descargar adb</button></div>` +
    `<div class="modal-hint">O indicá la ruta al tuyo (vacía = buscarlo automáticamente):</div>` +
    `<div class="art-search-row"><input type="text" data-dev="adb" placeholder="ej. C:\\platform-tools\\adb.exe o /usr/bin/adb">` +
    `<button class="cartridge-btn-secondary" data-dev="adbSave">Guardar y reintentar</button></div>`;
  $('[data-dev="download"]').onclick = async e => {
    e.target.disabled = true;
    e.target.textContent = "Descargando...";
    try {
      await invoke("android_download_adb");
      retry();
    } catch (err) {
      showAdbSetup(el, String(err), retry);
    }
  };
  $('[data-dev="adbSave"]').onclick = async () => {
    try {
      await invoke("android_set_adb_path", { path: $('[data-dev="adb"]').value });
      retry();
    } catch (err) {
      showAdbSetup(el, String(err), retry);
    }
  };
}
