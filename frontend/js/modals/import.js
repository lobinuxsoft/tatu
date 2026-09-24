import { invoke } from "../tauri.js";
import { state } from "../state.js";
import { esc } from "../utils.js";
import { renderSteam } from "../render/steam.js";

export async function openImportModal() {
  const overlay = document.getElementById("importOverlay");
  const list = document.getElementById("importList");
  const result = document.getElementById("importResult");
  result.innerHTML = "";
  list.innerHTML = '<div class="loading"><div class="spinner"></div><br>Leyendo colecciones...</div>';
  overlay.classList.remove("hidden");

  try {
    const collections = await invoke("list_steam_collections");
    const usable = (collections || [])
      .filter(c => c.name && c.name.trim() && c.added && c.added.length > 0)
      .sort((a, b) => b.added.length - a.added.length);

    if (usable.length === 0) {
      list.innerHTML = `<div class="loading" style="color:#8b949e">No se encontraron colecciones con juegos.</div>`;
      return;
    }

    let html = "";
    for (const c of usable) {
      html += `<div class="collection-row" data-name="${esc(c.name)}"><span class="collection-name">${esc(c.name)}</span><span class="collection-count">${c.added.length} juegos</span></div>`;
    }
    list.innerHTML = html;

    list.onclick = async e => {
      const row = e.target.closest(".collection-row");
      if (!row || row.classList.contains("disabled")) return;
      const name = row.dataset.name;
      list.querySelectorAll(".collection-row").forEach(r => r.classList.add("disabled"));
      result.innerHTML = `<div class="import-result" style="color:#8b949e">Importando "${esc(name)}"...</div>`;
      try {
        const [matched, unknown] = await invoke("import_completed_from_collection", { collectionName: name });
        const data = await invoke("get_state");
        state.completed = new Set(data.completed || []);
        renderSteam();
        const msg = `\u2713 ${matched} juegos marcados como completados desde "${esc(name)}".`
          + (unknown > 0 ? ` ${unknown} juegos de la colección no están en tu librería (ocultos o removidos).` : "");
        result.innerHTML = `<div class="import-result import-result-ok">${msg}</div>`;
      } catch (err) {
        result.innerHTML = `<div class="import-result import-result-err">Error: ${esc(String(err))}</div>`;
        list.querySelectorAll(".collection-row").forEach(r => r.classList.remove("disabled"));
      }
    };
  } catch (e) {
    list.innerHTML = `<div class="import-result import-result-err">Error: ${esc(String(e))}</div>`;
  }
}

export function closeImportModal() {
  document.getElementById("importOverlay").classList.add("hidden");
}
