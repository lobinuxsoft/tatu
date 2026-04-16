// Theme switcher. Persists the user's choice in localStorage and applies
// it by toggling `data-theme` on <html>. All tokens live in CSS, so the
// switch is instantaneous — no reload required.

export const THEMES = ["cyberpunk", "steamdeck", "heroic", "playnite"];
const STORAGE_KEY = "gpt-theme";
const DEFAULT = "cyberpunk";

function valid(theme) {
  return typeof theme === "string" && THEMES.includes(theme);
}

export function currentTheme() {
  const stored = localStorage.getItem(STORAGE_KEY);
  return valid(stored) ? stored : DEFAULT;
}

export function applyTheme(theme) {
  const t = valid(theme) ? theme : DEFAULT;
  document.documentElement.dataset.theme = t;
  try { localStorage.setItem(STORAGE_KEY, t); } catch (_) {}
}

/// Read the stored preference and apply it. Call from the app entry point
/// as early as possible to avoid a flash of the default palette.
export function initTheme() {
  applyTheme(currentTheme());
}

/// Wire the `<select id="themeSelect">` in the Settings tab to the applier.
export function installThemeSwitcher() {
  const sel = document.getElementById("themeSelect");
  if (!sel) return;
  sel.value = currentTheme();
  sel.addEventListener("change", e => applyTheme(e.target.value));
}
