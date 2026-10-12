// Every `.modal-overlay` as a dialog for the keyboard and screen readers,
// without touching each modal: role and label, focus moved in when it opens
// and back to where it was when it closes, Tab kept inside, Escape closes it
// through its own × button (so whatever that button does on close still runs).
// Modals open and close by toggling `.hidden` on the overlay.
//
// "Where it was" survives a re-render: when the control that opened the modal
// is gone by the time it closes (the PC tab redraws after an install), focus
// goes to its closest ancestor with an id — the same region of the page.

const FOCUSABLE = 'button, [href], input, select, textarea, summary, [tabindex]:not([tabindex="-1"])';

// What Tab actually stops on: enabled, visible, not taken out of the order,
// and one radio per group (the checked one) — the browser skips the rest.
function focusables(modal) {
  const skippedRadio = el =>
    el.type === "radio" &&
    !el.checked &&
    el.name &&
    [...modal.querySelectorAll('input[type="radio"]')].some(r => r.name === el.name && r.checked);
  return [...modal.querySelectorAll(FOCUSABLE)].filter(
    el => !el.disabled && el.offsetParent !== null && el.tabIndex >= 0 && !skippedRadio(el),
  );
}

export function installModalA11y() {
  const overlays = [...document.querySelectorAll(".modal-overlay")];
  const returnTo = new Map();
  // A button that disables itself while it works has already lost focus by
  // the time its modal opens, and its region is read while it's still on the
  // page: the observer below only runs after a re-render may have removed it.
  const region = el => {
    const anchor = el && el.closest("[id]");
    return anchor ? anchor.id : null;
  };
  // Somewhere focus can actually go back to: not a modal that closed since
  // (two of them swapping in one step), not an element that's gone.
  const usable = el => el && el.isConnected && el.offsetParent !== null && !el.closest(".modal-overlay.hidden");
  let lastFocused = { from: null, region: null };
  // The last spot outside every modal: where focus belongs when the opener
  // itself was in a modal that has closed since.
  let lastOutside = { from: null, region: null };
  document.addEventListener("focusin", e => {
    lastFocused = { from: e.target, region: region(e.target) };
    if (!e.target.closest(".modal-overlay")) lastOutside = lastFocused;
  });

  overlays.forEach((overlay, i) => {
    const modal = overlay.querySelector(".modal") || overlay;
    modal.setAttribute("role", "dialog");
    modal.setAttribute("aria-modal", "true");
    const title = modal.querySelector("h2, h3");
    if (title) {
      title.id ||= `modalTitle${i}`;
      modal.setAttribute("aria-labelledby", title.id);
    }
    modal.querySelectorAll(".modal-close").forEach(b => b.setAttribute("aria-label", "Cerrar"));

    new MutationObserver(() => {
      const open = !overlay.classList.contains("hidden");
      if (open && !returnTo.has(overlay)) {
        const active = document.activeElement;
        returnTo.set(
          overlay,
          usable(active) && active !== document.body ? { from: active, region: region(active) } : lastFocused,
        );
        // After the modal's own code fills it in.
        requestAnimationFrame(() => {
          const items = focusables(modal);
          (items.find(el => !el.classList.contains("modal-close")) || items[0])?.focus();
        });
      } else if (!open && returnTo.has(overlay)) {
        const { from, region } = returnTo.get(overlay);
        returnTo.delete(overlay);
        const back = [from, lastOutside.from].find(usable);
        if (back) {
          back.focus();
          return;
        }
        const area = [region, lastOutside.region]
          .map(id => id && document.getElementById(id))
          .find(usable);
        if (!area) return;
        if (!area.matches(FOCUSABLE) && !area.hasAttribute("tabindex")) {
          area.tabIndex = -1;
          area.addEventListener("blur", () => area.removeAttribute("tabindex"), { once: true });
        }
        area.focus({ preventScroll: true });
      }
    }).observe(overlay, { attributes: true, attributeFilter: ["class"] });
  });

  document.addEventListener("keydown", e => {
    const overlay = overlays.filter(o => !o.classList.contains("hidden")).pop();
    if (!overlay) return;
    const modal = overlay.querySelector(".modal") || overlay;
    if (e.key === "Escape") {
      e.preventDefault();
      const close = modal.querySelector(".modal-close");
      if (close) close.click();
      else overlay.classList.add("hidden");
    } else if (e.key === "Tab") {
      const items = focusables(modal);
      if (!items.length) return;
      const first = items[0];
      const last = items[items.length - 1];
      if (!modal.contains(document.activeElement)) {
        e.preventDefault();
        first.focus();
      } else if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
  });
}
