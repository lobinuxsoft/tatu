import { opener } from "./tauri.js";

// Schemes that belong in the user's browser or mail client rather than in a
// webview that has no back button and no address bar.
const EXTERNAL = ["http:", "https:", "mailto:"];

// Attach once on startup. Every anchor pointing outside the app is routed
// through the Tauri opener plugin, wherever in the document it lives and
// whenever it was rendered.
//
// The plugin's own click interceptor only fires for `target="_blank"` anchors
// or ctrl/shift-clicks, so plain in-app links were falling through to a real
// navigation — which stranded the window on an external site with no way back
// (the app has no chrome). Rust refuses that navigation as a backstop; this
// handler is what makes the link actually do something useful.
export function installExternalLinks() {
  document.addEventListener("click", e => {
    if (e.defaultPrevented || e.button !== 0) return;
    const a = e.target.closest("a[href]");
    if (!a) return;

    let url;
    try {
      url = new URL(a.href, window.location.href);
    } catch (_) {
      return;
    }
    // Same-origin links are the app's own navigation — the A-Z jump list is
    // `<a href="#g-A">`, and Tauri may serve the frontend over
    // http://tauri.localhost, so a scheme check alone would ship those to the
    // browser.
    if (url.origin === window.location.origin) return;
    if (!EXTERNAL.includes(url.protocol)) return;

    e.preventDefault();
    opener.openUrl(url.href).catch(err => console.error("openUrl failed", err));
  });
}
