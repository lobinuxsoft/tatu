// Shared mutable state. Modules import `state` and read/write its fields —
// a plain object wrapper is used so reassignments propagate across modules
// (which `export let` does not allow).
export const state = {
  G: [],
  NS: [],
  GOG: [],
  completed: new Set(),
  completedNS: new Set(),
  completedGog: new Set(),
  achProgress: {},
  tog: { game: true, mp: false, tool: false, demo: false },
  sf: "all",
  q: "",
  gogQ: "",
  favorites: new Set(),
  hltbCache: {},
  drmCache: {},
  sizeCache: {},
  sortMode: "alpha",
  pf: "all",
  panelOpen: false,
  panelGameId: null,
  hasConfig: false,
  // False on Windows until the Win32 memory backend lands (#181). The whole
  // Cheats tab is dropped rather than disabled: the commands behind it are
  // not registered in that build, so every control would throw on click.
  cheatsSupported: true,
};

export const TL = { tool: "Tool", mp: "MP", demo: "Demo" };
export const TC = { tool: "tag-tool", mp: "tag-mp", demo: "tag-demo" };
export const AL = [
  "#","A","B","C","D","E","F","G","H","I","J","K","L","M",
  "N","O","P","Q","R","S","T","U","V","W","X","Y","Z",
];
