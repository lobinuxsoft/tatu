// Names one download so its own Cancel button reaches it: several can run
// at once (another game, another file).
export function downloadToken() {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

export function esc(s) {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

export function fmtDate(epoch) {
  if (!epoch) return "";
  return new Date(epoch * 1000).toLocaleDateString("es-AR", {
    day: "numeric",
    month: "short",
    year: "numeric",
  });
}

export function mcColor(score) {
  return score >= 75 ? "mc-green" : score >= 50 ? "mc-yellow" : "mc-red";
}

export function formatBytes(n) {
  if (!n || n < 0) return "\u2014";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0, v = n;
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; }
  return (v < 10 && i >= 2 ? v.toFixed(1) : Math.round(v)) + " " + units[i];
}

export const gCat = g => g.tag || "game";

export const gLetter = n => {
  const f = n.charAt(0).toUpperCase();
  return /[A-Z]/.test(f) ? f : "#";
};
