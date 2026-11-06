// A stub browser provider: it answers primitives only (capability flags,
// clipboard text, raw queue events, dialog tokens and results). Every rule,
// decoded value and error the program prints is Core's. Multi-field answers
// are provider records: each field is `<UTF-8 byte length>:<bytes>`.
const encoder = new TextEncoder();
const record = (fields) => fields.map((field) => encoder.encode(field).length + ":" + field).join("");
const queues = {
  0: [
    record(["notes", "compositionupdate", "か\u241fな\x1f", "0", "2", "0", "2"]),
    record(["notes", "bogus", "", "", "", "", ""]),
    "9:short",
  ],
  1: [record(["notes", "drop", "copy", "hel\u241flo\x1f", "https://jet-lang.dev/"])],
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
  dialogResult: (token) => record(dialogs[token].kind === 0 ? ["selected", "notes/today.txt"] : ["cancelled"]),
};
const { jet_main } = await import("./app.js");
await jet_main();
