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

function open(img) {
  const box = ensureOverlay();
  const shown = box.querySelector(".lightbox-img");
  shown.src = img.src;
  shown.alt = img.alt || "";

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
