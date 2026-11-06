// A stub browser provider: it answers primitives only (capability flags,
// clipboard text, raw queue events, dialog tokens and results). Every rule,
// decoded value and error the program prints is Core's.
const field = "\x1f";
const item = "\x1e";
const queues = {
  0: [
    ["notes", "Update", "かな", "0", "2", "0", "2"].join(field),
    ["notes", "Bogus", "", "", "", "", ""].join(field),
  ],
  1: [["notes", "Drop", "Copy", "Thello", "Uhttps://jet-lang.dev/"].join(field)],
};
const dialogs = [];
let clipboard = "seed";
globalThis.jetUiProvider = {
  capability: (code) => code !== 6,
  clipboardGet: () => clipboard,
  clipboardSet: (text) => {
    clipboard = text;
    return true;
  },
  eventPop: (queue) => queues[queue].shift() ?? null,
  dialogRequest: (kind, request) => dialogs.push({ kind, request }) - 1,
  dialogResult: (token) =>
    dialogs[token].kind === 0 ? ["selected", "notes/today.txt"].join(item) : "cancelled",
};
const { jet_main } = await import("./app.js");
await jet_main();
