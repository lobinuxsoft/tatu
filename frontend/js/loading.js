const loadingTasks = new Set();

export function updateGlobalLoading() {
  const el = document.getElementById("globalLoading");
  if (!el) return;
  if (loadingTasks.size === 0) {
    el.classList.add("hidden");
  } else {
    el.classList.remove("hidden");
    const msgs = [];
    if (loadingTasks.has("info")) msgs.push("info del juego");
    if (loadingTasks.has("logros")) msgs.push("logros");
    if (loadingTasks.has("cromos")) msgs.push("cromos e insignias");
    el.querySelector(".global-loading-text").textContent = `Cargando ${msgs.join(", ")}...`;
  }
}

export function startLoading(task) {
  loadingTasks.add(task);
  updateGlobalLoading();
}

export function doneLoading(task) {
  loadingTasks.delete(task);
  updateGlobalLoading();
}

export function clearLoadingTasks() {
  loadingTasks.clear();
  updateGlobalLoading();
}
