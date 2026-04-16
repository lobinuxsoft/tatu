import { invoke } from "./tauri.js";
import { state } from "./state.js";

export function loadSettingsUI(apiKey, steamId) {
  document.getElementById("cfgApiKey").value = apiKey || "";
  document.getElementById("cfgSteamId").value = steamId || "";
  state.hasConfig = !!(apiKey && steamId);
}

export function checkConfigWarning() {
  const existing = document.getElementById("configWarning");
  if (existing) existing.remove();
  if (!state.hasConfig) {
    const warn = document.createElement("div");
    warn.id = "configWarning";
    warn.className = "config-warning";
    warn.innerHTML = 'Configurá tu Steam API Key y Steam ID en la pestaña <strong>Settings</strong> para poder sincronizar.';
    document.getElementById("panel-steam").prepend(warn);
  }
}

export function installSettingsHandlers() {
  document.getElementById("toggleKeyBtn").addEventListener("click", () => {
    const inp = document.getElementById("cfgApiKey");
    const btn = document.getElementById("toggleKeyBtn");
    if (inp.type === "password") { inp.type = "text"; btn.textContent = "Ocultar"; }
    else { inp.type = "password"; btn.textContent = "Mostrar"; }
  });

  document.getElementById("detectSteamIdBtn").addEventListener("click", async () => {
    const btn = document.getElementById("detectSteamIdBtn");
    btn.disabled = true; btn.textContent = "Detectando...";
    try {
      const id = await invoke("detect_steam_id");
      if (id) {
        document.getElementById("cfgSteamId").value = id;
        btn.textContent = "Detectado";
      } else {
        btn.textContent = "No encontrado";
      }
    } catch (_) {
      btn.textContent = "Error";
    }
    setTimeout(() => { btn.disabled = false; btn.textContent = "Detectar"; }, 3000);
  });

  document.getElementById("saveSettingsBtn").addEventListener("click", async () => {
    const key = document.getElementById("cfgApiKey").value.trim();
    const id = document.getElementById("cfgSteamId").value.trim();
    const msg = document.getElementById("settingsMsg");
    try {
      await invoke("save_settings", { steamApiKey: key, steamId: id });
      state.hasConfig = !!(key && id);
      checkConfigWarning();
      msg.style.color = "#2ea043";
      msg.textContent = "Guardado";
      setTimeout(() => { msg.textContent = ""; }, 3000);
    } catch (e) {
      msg.style.color = "#f85149";
      msg.textContent = "Error: " + e;
    }
  });
}
