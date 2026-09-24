// Steam tilts a card toward the pointer and lifts it off the page. This is
// that, in CSS transforms driven by pointer position: the element itself
// stays flat in the DOM, only `--rx`/`--ry` (rotation) and `--gx`/`--gy`
// (where the sheen sits) change.

const TILTABLE = ".card-item, .badge-item";
const MAX_DEG = 9;

let active = null;

function reset(el) {
  el.classList.remove("tilting");
  el.style.removeProperty("--rx");
  el.style.removeProperty("--ry");
  el.style.removeProperty("--gx");
  el.style.removeProperty("--gy");
}

function track(el, e) {
  const r = el.getBoundingClientRect();
  if (!r.width || !r.height) return;

  // -0.5 .. 0.5 from the centre of the element.
  const px = (e.clientX - r.left) / r.width - 0.5;
  const py = (e.clientY - r.top) / r.height - 0.5;

  // Pointer right tips the right edge away, pointer down tips the bottom
  // away — the sign flip on X is what makes it read as following the cursor
  // rather than fleeing it.
  el.style.setProperty("--ry", `${(px * MAX_DEG * 2).toFixed(2)}deg`);
  el.style.setProperty("--rx", `${(-py * MAX_DEG * 2).toFixed(2)}deg`);
  el.style.setProperty("--gx", `${((px + 0.5) * 100).toFixed(1)}%`);
  el.style.setProperty("--gy", `${((py + 0.5) * 100).toFixed(1)}%`);
}

export function installCardTilt() {
  // Respect the accessibility setting: this is decoration, not information.
  if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) return;

  document.addEventListener("pointermove", e => {
    const el = e.target.closest(TILTABLE);
    if (el !== active) {
      if (active) reset(active);
      active = el;
      if (el) el.classList.add("tilting");
    }
    if (el) track(el, e);
  });

  // Leaving through a gap, a scroll, or the window edge all have to settle
  // the card back down, so reset on more than pointerout.
  const settle = () => { if (active) { reset(active); active = null; } };
  document.addEventListener("pointerleave", settle);
  document.addEventListener("scroll", settle, true);
  window.addEventListener("blur", settle);
}
