import { invoke, opener } from "./tauri.js";
import { state } from "./state.js";
import { renderGog } from "./render/gog.js";

export function loadSettingsUI(apiKey, steamId, sgdbApiKey, pcgwUsername, pcgwBotPassword, gogConnected) {
  document.getElementById("cfgApiKey").value = apiKey || "";
  document.getElementById("cfgSteamId").value = steamId || "";
  document.getElementById("cfgSgdbApiKey").value = sgdbApiKey || "";
  document.getElementById("cfgPcgwUsername").value = pcgwUsername || "";
  document.getElementById("cfgPcgwBotPassword").value = pcgwBotPassword || "";
  state.hasConfig = !!(apiKey && steamId);
  renderGogConnectionState(!!gogConnected);
}

function renderGogConnectionState(connected) {
  document.getElementById("gogDisconnected").style.display = connected ? "none" : "";
  document.getElementById("gogConnected").style.display = connected ? "" : "none";
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

  document.getElementById("toggleSgdbKeyBtn").addEventListener("click", () => {
    const inp = document.getElementById("cfgSgdbApiKey");
    const btn = document.getElementById("toggleSgdbKeyBtn");
    if (inp.type === "password") { inp.type = "text"; btn.textContent = "Ocultar"; }
    else { inp.type = "password"; btn.textContent = "Mostrar"; }
  });

  document.getElementById("togglePcgwBotPasswordBtn").addEventListener("click", () => {
    const inp = document.getElementById("cfgPcgwBotPassword");
    const btn = document.getElementById("togglePcgwBotPasswordBtn");
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
    const sgdbKey = document.getElementById("cfgSgdbApiKey").value.trim();
    const pcgwUsername = document.getElementById("cfgPcgwUsername").value.trim();
    const pcgwBotPassword = document.getElementById("cfgPcgwBotPassword").value.trim();
    const msg = document.getElementById("settingsMsg");
    try {
      await invoke("save_settings", {
        steamApiKey: key, steamId: id, steamgriddbApiKey: sgdbKey,
        pcgwUsername, pcgwBotPassword,
      });
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

  document.getElementById("gogConnectBtn").addEventListener("click", async () => {
    const url = await invoke("gog_login_url");
    opener.openUrl(url).catch(err => console.error("openUrl failed", err));
  });

  document.getElementById("gogSubmitCodeBtn").addEventListener("click", async () => {
    const pasted = document.getElementById("gogPastedCode").value.trim();
    const msg = document.getElementById("gogConnectMsg");
    const btn = document.getElementById("gogSubmitCodeBtn");
    if (!pasted) return;
    btn.disabled = true;
    try {
      await invoke("gog_connect", { pasted });
      document.getElementById("gogPastedCode").value = "";
      msg.style.color = "#2ea043";
      msg.textContent = "Conectado.";
      renderGogConnectionState(true);
    } catch (e) {
      msg.style.color = "#f85149";
      msg.textContent = "Error: " + e;
    }
    btn.disabled = false;
  });

  document.getElementById("gogDisconnectBtn").addEventListener("click", async () => {
    await invoke("gog_disconnect");
    renderGogConnectionState(false);
    state.GOG = [];
    renderGog();
  });
}
