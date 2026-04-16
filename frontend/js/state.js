// Shared mutable state. Modules import `state` and read/write its fields —
// a plain object wrapper is used so reassignments propagate across modules
// (which `export let` does not allow).
export const state = {
  G: [],
  NS: [],
  completed: new Set(),
  completedNS: new Set(),
  achProgress: {},
  tog: { game: true, mp: false, tool: false, demo: false },
  sf: "all",
  q: "",
  favorites: new Set(),
  hltbCache: {},
  drmCache: {},
  sizeCache: {},
  sortMode: "alpha",
  pf: "all",
  panelOpen: false,
  panelGameId: null,
  hasConfig: false,
};

export const TL = { tool: "Tool", mp: "MP", demo: "Demo" };
export const TC = { tool: "tag-tool", mp: "tag-mp", demo: "tag-demo" };
export const AL = [
  "#","A","B","C","D","E","F","G","H","I","J","K","L","M",
  "N","O","P","Q","R","S","T","U","V","W","X","Y","Z",
];
