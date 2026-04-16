import { state } from "../state.js";
import { esc } from "../utils.js";

export function renderNonSteam() {
  const content = document.getElementById("nsContent");
  document.getElementById("tabNonsteamCount").textContent = "(" + state.NS.length + ")";

  if (state.NS.length === 0) {
    content.innerHTML = '<div class="empty-state">No hay juegos Non-Steam. Dale a "Leer shortcuts.vdf".</div>';
    document.getElementById("nsComp").textContent = "0";
    document.getElementById("nsPend").textContent = "0";
    document.getElementById("nspBar").style.width = "0%";
    document.getElementById("nspText").textContent = "0%";
    return;
  }

  let comp = 0;
  let rows = "";
  state.NS.forEach(g => {
    const chk = state.completedNS.has(g.id);
    if (chk) comp++;
    const exePath = g.exe ? `<div class="exe-path" title="${esc(g.exe)}">${esc(g.exe)}</div>` : "";
    rows += `<tr class="${chk ? "done" : ""}"><td><input type="checkbox" data-id="${g.id}" data-list="nonsteam" ${chk ? "checked" : ""}></td><td><div class="game-cell"><span class="gn">${esc(g.name)}</span>${exePath}</div></td><td>\u2014</td></tr>`;
  });

  const total = state.NS.length;
  const pct = Math.round(comp / total * 100);
  content.innerHTML = `<table><thead><tr><th></th><th>Juego</th><th>Horas</th></tr></thead><tbody>${rows}</tbody></table>`;
  document.getElementById("nsComp").textContent = comp;
  document.getElementById("nsPend").textContent = total - comp;
  document.getElementById("nspBar").style.width = pct + "%";
  document.getElementById("nspText").textContent = pct + "% (" + comp + "/" + total + ")";
}
