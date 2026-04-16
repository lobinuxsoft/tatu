import { esc } from "../utils.js";
import { opener } from "../tauri.js";

function drmStatusLabel(info) {
  if (!info || !info.status) return { icon: "", label: "DRM: Desconocido", cls: "tag-drm-unknown" };
  const k = info.status.kind;
  if (k === "drm_free") return { icon: "\u{1F513}", label: "DRM: Ninguno", cls: "tag-drm-free" };
  if (k === "steam_only") return { icon: "\u{1F6E1}", label: "DRM: Solo Steam", cls: "tag-drm-steam" };
  if (k === "third_party") {
    const vendors = (info.status.vendors || []).map(v => esc(v)).join(", ");
    return { icon: "\u{1F512}", label: "DRM: " + (vendors || "desconocido"), cls: "tag-drm-third" };
  }
  return { icon: "", label: "DRM: Desconocido", cls: "tag-drm-unknown" };
}

function drmImpactTooltip(info) {
  if (!info) return "";
  const impact = info.affects_steam_copy
    ? "[Afecta tu copia de Steam] "
    : (info.status && info.status.kind === "drm_free" ? "[Tu copia de Steam es libre] " : "");
  const explanation = info.explanation || info.notes || "";
  return impact + explanation;
}

export function renderDrmBadge(info) {
  const { icon, label, cls } = drmStatusLabel(info);
  const inner = icon ? `${icon} ${label}` : label;
  return `<span class="tag ${cls}">${inner}</span>`;
}

export function renderDrmInlineBadge(info) {
  if (!info || !info.status) return "";
  const k = info.status.kind;
  if (k === "unknown") return "";
  const { icon, label, cls } = drmStatusLabel(info);
  const tip = drmImpactTooltip(info);
  const tipAttr = tip ? ` title="${esc(tip)}"` : "";
  return `<span class="drm-inline tag ${cls}"${tipAttr}>${icon} ${label}</span>`;
}

export function renderDrmExplanation(info) {
  if (!info) return "";
  const affects = info.affects_steam_copy;
  const cls = affects
    ? "drm-impact-warn"
    : (info.status && info.status.kind === "drm_free" ? "drm-impact-ok" : "drm-impact-muted");
  const heading = affects
    ? "\u26A0 Afecta tu copia de Steam"
    : (info.status && info.status.kind === "drm_free" ? "\u2705 Tu copia de Steam es libre de DRM" : "\u2139 Impacto desconocido");
  const text = info.explanation || info.notes || "";
  return `<div class="drm-impact ${cls}"><div class="drm-impact-heading">${heading}</div><div class="drm-impact-text">${esc(text)}</div></div>`;
}

function preservabilityInfo(info) {
  if (!info || !info.preservability || !info.preservability.kind) {
    return { key: "unknown", heading: "? Preservabilidad desconocida", cls: "pres-unknown" };
  }
  const k = info.preservability.kind;
  if (k === "trivial") return { key: "trivial", heading: "\u2705 Preservación trivial", cls: "pres-trivial" };
  if (k === "easy") return { key: "easy", heading: "\u2705 Compatible con Goldberg Emulator", cls: "pres-easy" };
  if (k === "alternative") return { key: "alternative", heading: "\u2139 Disponible DRM-free en GOG", cls: "pres-alternative" };
  if (k === "removed") return { key: "removed", heading: "\u2705 DRM removido oficialmente", cls: "pres-removed" };
  if (k === "hard") return { key: "hard", heading: "\u26A0 Preservación compleja", cls: "pres-hard" };
  return { key: "unknown", heading: "? Preservabilidad desconocida", cls: "pres-unknown" };
}

function gogSearchUrl(name) {
  return "https://www.gog.com/en/games?query=" + encodeURIComponent(name || "");
}

export function renderPreservabilityBlock(info, gameName) {
  if (!info) return "";
  const p = preservabilityInfo(info);
  const hint = info.preservability_hint || "";
  let extra = "";
  if (p.key === "alternative" && gameName) {
    const url = gogSearchUrl(gameName);
    extra = `<div class="pres-action"><a href="${esc(url)}" data-gog-url="${esc(url)}">\u{1F517} Buscar en GOG</a></div>`;
  }
  return `<div class="pres-block ${p.cls}"><div class="pres-heading">${p.heading}</div><div class="pres-text">${esc(hint)}</div>${extra}</div>`;
}

// Attach once on startup: open GOG search URLs via the Tauri opener plugin
// when the user clicks an `a[data-gog-url]` anchor anywhere in the document.
export function installGogLinkHandler() {
  document.addEventListener("click", e => {
    const a = e.target.closest("a[data-gog-url]");
    if (!a) return;
    e.preventDefault();
    const url = a.getAttribute("data-gog-url");
    if (url) opener.openUrl(url);
  });
}
