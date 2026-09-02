// #1414 peer adapter. Existing authorized web incumbent:
// TypeScript/ECMAScript, commit 5be33469d551655d878876faa9e30aa3b49f8ee9.
// The implementation below is independent: the revision identifies the
// selected web boundary, not copied adapter source.

const ACTIONS = new Set([
  "add",
  "edit",
  "search",
  "save",
  "reload",
  "readonly",
  "corrupt",
]);
const FIELDS = new Set(["title", "body", "query"]);

function newModel() {
  return {
    notes: [],
    active: -1,
    focus: "title",
    query: "",
    search: "not-run",
    persistence: "not-saved",
    readonly: "not-run",
    corrupt: "not-run",
    readOnlyStorage: false,
    savedNotes: null,
    unknown: false,
  };
}

function cloneNotes(notes) {
  return notes.map((note) => ({ title: note.title, body: note.body }));
}

function parseLine(line) {
  const separator = line.indexOf(":");
  if (separator < 0) return { kind: "unknown" };

  const name = line.slice(0, separator);
  const value = line.slice(separator + 1);
  if (name === "key" && ACTIONS.has(value)) {
    return { kind: "action", name: value };
  }
  if (FIELDS.has(name)) return { kind: "field", name, value };
  return { kind: "unknown" };
}

function updateSearch(model) {
  model.search = model.notes.some(
    (note) =>
      note.title.includes(model.query) || note.body.includes(model.query),
  )
    ? "found"
    : "not-found";
}

function applyAction(model, name) {
  switch (name) {
    case "add":
      model.notes.push({ title: "", body: "" });
      model.active = model.notes.length - 1;
      model.focus = "title";
      break;
    case "edit":
      if (model.notes.length > 0) model.active = model.notes.length - 1;
      model.focus = "title";
      break;
    case "search":
      updateSearch(model);
      break;
    case "save":
      if (!model.readOnlyStorage) {
        model.savedNotes = cloneNotes(model.notes);
        model.persistence = "saved";
      }
      break;
    case "reload":
      if (model.savedNotes !== null) {
        model.notes = cloneNotes(model.savedNotes);
        model.active = model.notes.length - 1;
        model.persistence = "reloaded";
      }
      break;
    case "readonly":
      model.readOnlyStorage = true;
      model.readonly = "blocked";
      break;
    case "corrupt":
      model.corrupt = "rejected";
      break;
    default:
      model.unknown = true;
  }
}

function applyEvent(model, event) {
  if (event.kind === "unknown") {
    model.unknown = true;
    return;
  }
  if (event.kind === "action") {
    applyAction(model, event.name);
    return;
  }

  if (event.name === "query") {
    model.query = event.value;
    if (model.search !== "not-run") updateSearch(model);
    return;
  }
  if (model.active >= 0) {
    model.notes[model.active][event.name] = event.value;
    if (model.search !== "not-run") updateSearch(model);
  }
}

export function renderNotes(raw) {
  const model = newModel();
  for (const line of String(raw ?? "").split(/\r?\n/)) {
    if (line.length > 0) applyEvent(model, parseLine(line));
  }

  const lines = [
    `notes=${model.notes.length}`,
    `focus=${model.focus}`,
    `search=${model.search}`,
    `persistence=${model.persistence}`,
    `readonly=${model.readonly}`,
    `corrupt=${model.corrupt}`,
  ];
  if (model.unknown) lines.push("reject=unknown-key");
  return `${lines.join("\n")}\n`;
}

const runningAsCli =
  typeof process !== "undefined" &&
  process.argv[1]?.endsWith("cross-platform-notes.mjs");

if (runningAsCli) {
  const fs = await import("node:fs");
  process.stdout.write(renderNotes(fs.readFileSync(process.argv[2], "utf8")));
} else if (typeof window !== "undefined") {
  for (const line of renderNotes(globalThis.__compiledWorkloadInput).trimEnd().split("\n")) {
    console.log(line);
  }
}
