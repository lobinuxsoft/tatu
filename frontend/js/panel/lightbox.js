// Cards and badges render at thumbnail size, which is too small to actually
// look at the art. Clicking one opens it full size over the window.

const ZOOMABLE = "img.card-img, img.badge-img";

let overlay = null;

function ensureOverlay() {
  if (overlay) return overlay;
  overlay = document.createElement("div");
  overlay.className = "lightbox hidden";
  overlay.innerHTML =
    `<img class="lightbox-img" alt="">` +
    `<div class="lightbox-caption"></div>`;
  overlay.addEventListener("click", close);
  document.body.appendChild(overlay);
  return overlay;
}

function close() {
  if (overlay) overlay.classList.add("hidden");
}

// How far a thumbnail may be blown up before it stops being a bigger picture
// and starts being bigger pixels. Cards are 224x261 masters and take the full
// budget; badges are 80x80 and would turn to mush at screen height.
const MAX_UPSCALE = 3;

function sizeToSource(shown) {
  const h = shown.naturalHeight;
  if (!h) return;
  shown.style.height = `min(70vh, ${h * MAX_UPSCALE}px)`;
}

function open(img) {
  const box = ensureOverlay();
  const shown = box.querySelector(".lightbox-img");
  shown.style.removeProperty("height");
  shown.onload = () => sizeToSource(shown);
  shown.src = img.src;
  shown.alt = img.alt || "";
  // A cached image may already be complete, in which case onload never fires.
  if (shown.complete) sizeToSource(shown);

  // The name sits in a sibling node of the thumbnail, not on the image.
  const label = img.closest(".card-item, .badge-item");
  const name = label?.querySelector(".card-name, .badge-name")?.textContent || "";
  box.querySelector(".lightbox-caption").textContent = name.trim();

  box.classList.remove("hidden");
}

export function installLightbox() {
  document.addEventListener("click", e => {
    const img = e.target.closest(ZOOMABLE);
    if (!img) return;
    e.preventDefault();
    open(img);
  });

  document.addEventListener("keydown", e => {
    if (e.key === "Escape") close();
  });
}
