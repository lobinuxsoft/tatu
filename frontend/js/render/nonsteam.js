import { state } from "../state.js";
import { renderSimpleGameList } from "./simple_list.js";

export function renderNonSteam() {
  renderSimpleGameList({
    items: state.NS,
    completedSet: state.completedNS,
    listKey: "nonsteam",
    countEl: document.getElementById("tabNonsteamCount"),
    contentEl: document.getElementById("nsContent"),
    compEl: document.getElementById("nsComp"),
    pendEl: document.getElementById("nsPend"),
    barEl: document.getElementById("nspBar"),
    textEl: document.getElementById("nspText"),
    emptyHtml: '<div class="empty-state">No hay juegos Non-Steam. Dale a "Leer shortcuts.vdf".</div>',
    subtitleOf: g => g.exe,
  });
}
