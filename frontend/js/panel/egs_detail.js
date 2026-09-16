import { getCurrentWindow } from "../tauri.js";
import { esc } from "../utils.js";
import { artworkTabHtml, installArtworkTabHandlers } from "./artwork_tab.js";
import {
  detailHeaderShell,
  headerImg,
  infoRow,
  detailTabsShell,
  installDetailTabSwitcher,
} from "./detail_template.js";

// Same shared window template Steam/GOG's detail views use (#243). Just an
// "Info" tab plus "Arte" — no download/cartridge button yet (#344/#345
// haven't landed), no Logros/Cromos/Cheats (real Steam APIs GOG/EGS have no
// equivalent of).
export function renderEgsDetail(game) {
  let infoHtml = "";
  if (game.developers && game.developers.length) {
    infoHtml += infoRow("Developer", esc(game.developers.join(", ")));
  }
  infoHtml += infoRow("Plataforma", "Epic Games Store");
  infoHtml += infoRow("App name", esc(game.app_name));
  if (game.description) {
    infoHtml +=
      `<details class="detail-desc-toggle" open><summary>Descripción</summary>` +
      `<div class="detail-desc" style="white-space:pre-wrap;">${esc(game.description)}</div></details>`;
  }

  document.getElementById("detailContent").innerHTML =
    detailHeaderShell(game.title, `<span>Epic Games Store</span>`, headerImg(game.background_url)) +
    detailTabsShell([
      { key: "info", label: "Info", initialHtml: infoHtml },
      { key: "arte", label: "Arte", initialHtml: artworkTabHtml() },
    ]);
  installDetailTabSwitcher();
  installArtworkTabHandlers(game.id, game.title);

  getCurrentWindow().setTitle(game.title + " — Tatu").catch(() => {});
}
