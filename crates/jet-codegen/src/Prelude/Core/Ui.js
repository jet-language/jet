// D-FOUND-PLATFORM1=A / card #2792: one Web adapter for the canonical UI
// value and HostServices wire. Browser code marshals DOM facts here; it does
// not reinterpret Core calls or create a second permission model.

const JET_UI_WEB_MAX_QUEUED_EVENTS = 256;
const JET_UI_WEB_CAPABILITIES = [
  "FileDialog",
  "Clipboard",
  "Ime",
  "DragDrop",
  "Shortcuts",
  "Accessibility",
];

function jet_ui_web_enum(tag, ...values) {
  return { tag, values };
}

function jet_ui_web_some(value) {
  return jet_option_some(value);
}

function jet_ui_web_none() {
  return jet_option_none();
}

function jet_ui_web_option(value) {
  return value == null ? jet_ui_web_none() : jet_ui_web_some(value);
}

function jet_ui_web_option_value(value) {
  if (value == null || value.tag === "Err") return undefined;
  if (value.tag === "Ok") return value.values?.[0];
  return value;
}

function jet_ui_web_ok(value) {
  return { tag: "Ok", values: [value] };
}

function jet_ui_web_err(error) {
  return { tag: "Err", values: [error] };
}


function jet_ui_web_cancelled(reason = "Headless") {
  return jet_ui_web_err(jet_ui_web_error("Cancelled", jet_ui_web_enum(reason)));
}



function jet_ui_web_headless() {
  return globalThis.__jetWebHeadless === true || typeof document === "undefined";
}

function jet_ui_web_default_capabilities() {
  if (jet_ui_web_headless()) {
    return Object.fromEntries(JET_UI_WEB_CAPABILITIES.map((name) => [name, true]));
  }
  const clipboard = globalThis.__jetUiClipboard;
  return {
    // Browser file paths cannot become JetUiGrantedPath values without a
    // package-provided, scoped picker adapter. Never pretend a DOM File has a
    // native path or silently widen its grant.
    FileDialog: typeof globalThis.__jetUiFileDialog === "function",
    Clipboard: clipboard != null
      && typeof clipboard.readText === "function"
      && typeof clipboard.writeText === "function",
    Ime: true,
    DragDrop: true,
    Shortcuts: true,
    Accessibility: true,
  };
}

function jet_ui_web_host_state() {
  let state = globalThis.__jetUiHostState;
  if (state == null || typeof state !== "object") {
    state = {
      clipboard: "",
      ime: [],
      drag: [],
      shortcuts: new Map(),
      shortcutListener: false,
      capabilities: null,
    };
    globalThis.__jetUiHostState = state;
  }
  if (!Array.isArray(state.ime)) state.ime = [];
  if (!Array.isArray(state.drag)) state.drag = [];
  if (!(state.shortcuts instanceof Map)) state.shortcuts = new Map();
  if (state.capabilities == null) state.capabilities = jet_ui_web_default_capabilities();
  return state;
}

function jet_ui_web_capability_flags() {
  const state = jet_ui_web_host_state();
  const override = globalThis.__jetUiCapabilities;
  if (override != null && typeof override === "object") {
    for (const name of JET_UI_WEB_CAPABILITIES) {
      if (name in override) state.capabilities[name] = Boolean(override[name]);
    }
  }
  return state.capabilities;
}

function jet_ui_web_capability(name) {
  return jet_ui_web_enum(name);
}

function jet_ui_web_capabilities() {
  return {
    facts: JET_UI_WEB_CAPABILITIES.map((name) => ({
      capability: jet_ui_web_capability(name),
      granted: Boolean(jet_ui_web_capability_flags()[name]),
    })),
  };
}

function jet_ui_host_capabilities() {
  return jet_ui_web_capabilities();
}

function jet_ui_web_require(name) {
  if (jet_ui_web_capability_flags()[name]) return null;
  return jet_ui_web_err(jet_ui_web_error("CapabilityDenied", jet_ui_web_capability(name)));
}

function jet_ui_web_number(value, fallback = 0) {
  const number = Number(value);
  return Number.isFinite(number) ? number : fallback;
}

function jet_ui_web_text(value) {
  return String(value ?? "");
}

function jet_ui_web_scalar_count(value) {
  return Array.from(String(value ?? "")).length;
}

function jet_ui_web_role(role) {
  if (role == null) return undefined;
  if (role.tag != null) return role;
  const names = { button: "Button", textbox: "TextInput", label: "Label", group: "Container" };
  return jet_ui_web_enum(names[String(role)] ?? String(role));
}

function jet_ui_web_kind(kind) {
  if (kind == null) return jet_ui_web_enum("Custom");
  if (kind.tag != null) return kind;
  const names = { custom: "Custom", text: "Text", box: "Box", button: "Button", textInput: "TextInput" };
  return jet_ui_web_enum(names[String(kind)] ?? String(kind));
}

function jet_ui_web_node(label, width, height, kind = "Custom", role, color) {
  return {
    label: jet_ui_web_text(label),
    width: jet_ui_web_number(width),
    height: jet_ui_web_number(height),
    role: jet_ui_web_option(jet_ui_web_role(role)),
    accessibility: jet_ui_web_none(),
    ime: jet_ui_web_none(),
    color: jet_ui_web_option(color == null ? undefined : jet_ui_web_text(color)),
    kind: jet_ui_web_kind(kind),
    children: [],
    key: jet_ui_web_none(),
    // These are runtime callback slots, not additional Core semantics. The
    // DOM adapter consumes them while the typed node fields remain canonical.
    on_click: null,
    on_drop: null,
    shortcut: jet_ui_web_none(),
  };
}

function jet_ui_null() {
  return { kind: "headless", commands: [], frame: null, node: null };
}

function jet_ui_tui() {
  return { kind: "headless", commands: [], frame: null, node: null };
}

function jet_ui_gtk() {
  if (typeof jetDom !== "undefined" && typeof jetDom.createBackend === "function") {
    return jetDom.createBackend();
  }
  return jet_ui_null();
}

function jet_ui_point(x, y) {
  return { x: jet_ui_web_number(x), y: jet_ui_web_number(y) };
}

function jet_ui_size(width, height) {
  return { width: jet_ui_web_number(width), height: jet_ui_web_number(height) };
}

function jet_ui_rect(x, y, width, height) {
  return {
    x: jet_ui_web_number(x),
    y: jet_ui_web_number(y),
    width: jet_ui_web_number(width),
    height: jet_ui_web_number(height),
  };
}

function jet_ui_constraint(minWidth, minHeight, maxWidth, maxHeight) {
  return {
    min_width: jet_ui_web_number(minWidth),
    min_height: jet_ui_web_number(minHeight),
    max_width: jet_ui_web_number(maxWidth),
    max_height: jet_ui_web_number(maxHeight),
  };
}

function jet_ui_node(label, width, height) {
  return jet_ui_web_node(label, width, height);
}

function jet_ui_node_role(label, width, height, role) {
  const canonicalRole = jet_ui_web_role(role);
  const kind = canonicalRole?.tag === "Button"
    ? "Button"
    : canonicalRole?.tag === "TextInput" ? "TextInput" : "Custom";
  return jet_ui_web_node(label, width, height, kind, canonicalRole);
}

function jet_ui_node_color(label, width, height, color) {
  return jet_ui_web_node(label, width, height, "Custom", jet_ui_web_enum("Label"), color);
}

function jet_ui_text(text) {
  const label = jet_ui_web_text(text);
  return jet_ui_web_node(label, jet_ui_web_scalar_count(label), 1, "Text", jet_ui_web_enum("Label"));
}

function jet_ui_button(label) {
  const text = jet_ui_web_text(label);
  return jet_ui_web_node(text, jet_ui_web_scalar_count(text) + 4, 1, "Button", jet_ui_web_enum("Button"));
}

function jet_ui_button_on_click(label, shortcut, accessibleLabel, handler) {
  const node = jet_ui_button(label);
  const shortcutValue = jet_ui_web_option_value(shortcut);
  const labelValue = jet_ui_web_option_value(accessibleLabel);
  if (shortcutValue !== undefined) node.shortcut = jet_ui_web_some(shortcutValue);
  if (labelValue !== undefined) {
    node.accessibility = jet_ui_web_some({
      name: jet_ui_web_some(jet_ui_web_text(labelValue)),
      description: jet_ui_web_none(),
      states: [],
    });
  }
  node.on_click = typeof handler === "function" ? handler : null;
  return node;
}

function jet_ui_box(children) {
  const list = Array.from(children ?? []);
  return {
    ...jet_ui_web_node("", list.reduce((width, child) => Math.max(width, jet_ui_web_number(child?.width)), 0), list.reduce((height, child) => height + jet_ui_web_number(child?.height), 0), "Box", jet_ui_web_enum("Container")),
    children: list,
  };
}

function jet_ui_reactive_render(body) {
  if (typeof body !== "function") throw new Error("core.ui.reactive_render requires a callback");
  body();
  return null;
}

// D-UI-PREVIEW1=A: named previews/playgrounds retain the canonical node
// callback and explicit viewport without introducing a second UI tree.
function jet_ui_preview_with_viewport(name, viewport, callback) {
  const previewName = jet_ui_web_text(name).trim();
  if (!previewName) throw new Error("core.ui.preview requires a non-empty name");
  if (typeof callback !== "function") {
    throw new Error("core.ui.preview requires a callback");
  }
  return {
    kind: "Preview",
    name: previewName,
    viewport: jet_ui_web_option_value(viewport) ?? jet_ui_web_enum("Desktop"),
    callback,
  };
}

function jet_ui_playground_with_viewport(name, viewport, callback) {
  const preview = jet_ui_preview_with_viewport(name, viewport, callback);
  return { ...preview, kind: "Playground" };
}

function jet_ui_previews(values) {
  return { kind: "PreviewRegistry", previews: Array.from(values ?? []) };
}

function jet_ui_playgrounds(values) {
  return { kind: "PreviewRegistry", previews: Array.from(values ?? []) };
}

function jet_ui_phone() {
  return jet_ui_web_enum("Phone");
}

function jet_ui_tablet() {
  return jet_ui_web_enum("Tablet");
}

function jet_ui_desktop() {
  return jet_ui_web_enum("Desktop");
}

function jet_ui_preview_attach_compiler_source(
  preview,
  sourceId,
  sourceFile,
  buildId,
  revision,
  startLine,
  startColumn,
  endLine,
  endColumn
) {
  return {
    ...preview,
    source: {
      source_id: sourceId,
      file: sourceFile,
      build_id: buildId,
      revision,
      span: {
        source_id: sourceId,
        file: sourceFile,
        start_line: startLine,
        start_column: startColumn,
        end_line: endLine,
        end_column: endColumn,
      },
    },
  };
}

function jet_ui_key_event(code) {
  return jet_ui_web_enum("Key", jet_ui_web_text(code));
}

function jet_ui_resize_event(width, height) {
  return jet_ui_web_enum("Resize", jet_ui_size(width, height));
}

function jet_ui_aria_role_button() {
  return jet_ui_web_enum("Button");
}

function jet_ui_aria_role_text_input() {
  return jet_ui_web_enum("TextInput");
}

function jet_ui_aria_role_label() {
  return jet_ui_web_enum("Label");
}

function jet_ui_aria_role_container() {
  return jet_ui_web_enum("Container");
}

function jet_ui_node_accessibility(node, accessibility) {
  return { ...node, accessibility: jet_ui_web_some(accessibility) };
}

function jet_ui_node_shortcut(node, shortcut) {
  return { ...node, shortcut: jet_ui_web_some(shortcut) };
}

function jet_ui_text_input(state, ime) {
  const text = jet_ui_web_text(state);
  const node = jet_ui_web_node(text, jet_ui_web_scalar_count(text), 1, "TextInput", jet_ui_web_enum("TextInput"));
  node.ime = jet_ui_web_some(ime);
  return node;
}

function jet_ui_text_input_on_drop(state, ime, handler) {
  const node = jet_ui_text_input(state, ime);
  node.on_drop = typeof handler === "function" ? handler : null;
  return node;
}

function jet_ui_node_id(value) {
  const text = jet_ui_web_text(value);
  if (text.length === 0) throw new Error("UI node identity must not be empty");
  return { value: text };
}

function jet_ui_text_range(start, end) {
  const first = Number(start);
  const last = Number(end);
  if (!Number.isSafeInteger(first) || !Number.isSafeInteger(last) || first < 0 || first > last) {
    throw new Error("text range start must not exceed end");
  }
  return { start: first, end: last };
}

function jet_ui_web_normalize_path(value) {
  const source = jet_ui_web_text(value).trim();
  if (source.length === 0) throw jet_ui_web_error("InvalidRequest", "filesystem grant path must not be empty");
  const rooted = source.startsWith("/");
  const parts = [];
  for (const part of source.replaceAll("\\", "/").split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") {
      if (parts.length === 0) throw jet_ui_web_error("ResourceDenied", `path \`${source}\` escapes its lexical root`);
      parts.pop();
    } else {
      parts.push(part);
    }
  }
  if (parts.length === 0) return rooted ? "/" : ".";
  return `${rooted ? "/" : ""}${parts.join("/")}`;
}

function jet_ui_web_rights_bits(rights) {
  return Number(rights?.bits ?? rights ?? 0) & 3;
}

function jet_ui_web_access_tag(access) {
  return access?.tag ?? String(access ?? "");
}

function jet_ui_web_grant_scope(grant, path, access) {
  const accessTag = jet_ui_web_access_tag(access);
  const bit = accessTag === "Read" ? 1 : accessTag === "Write" ? 2 : 0;
  if ((jet_ui_web_rights_bits(grant?.rights) & bit) === 0) {
    return jet_ui_web_err(jet_ui_web_error("ResourceDenied", `${grant?.root ?? "<grant>"} grant does not allow ${accessTag}`));
  }
  const root = jet_ui_web_normalize_path(grant.root);
  const requested = jet_ui_web_text(path);
  const candidate = jet_ui_web_normalize_path(requested.startsWith("/") ? requested : `${root}/${requested}`);
  const within = root === "."
    ? !candidate.startsWith("/")
    : root === "/" ? candidate.startsWith("/") : candidate === root || candidate.startsWith(`${root}/`);
  if (!within) {
    return jet_ui_web_err(jet_ui_web_error("ResourceDenied", `path \`${requested}\` escapes grant \`${root}\``));
  }
  return jet_ui_web_ok({
    path: candidate,
    grant_root: root,
    access: jet_ui_web_enum(accessTag),
  });
}

function jet_ui_host_file_filter(label, extensions, mimeTypes) {
  const extensionList = Array.from(extensions ?? []).map(jet_ui_web_text);
  const mimeList = Array.from(mimeTypes ?? []).map(jet_ui_web_text);
  const name = jet_ui_web_text(label);
  if (name.length === 0 && extensionList.length === 0 && mimeList.length === 0) {
    return jet_ui_web_err(jet_ui_web_error("InvalidRequest", "file filter must name an extension, media type, or label"));
  }
  if (extensionList.some((value) => value.length === 0)) {
    return jet_ui_web_err(jet_ui_web_error("InvalidRequest", "file filter extensions must not be empty"));
  }
  if (mimeList.some((value) => value.length === 0)) {
    return jet_ui_web_err(jet_ui_web_error("InvalidRequest", "file filter media types must not be empty"));
  }
  return jet_ui_web_ok({ label: name, extensions: extensionList, mime_types: mimeList });
}

function jet_ui_host_file_filter_text() {
  return { label: "Text", extensions: ["txt"], mime_types: ["text/plain"] };
}

function jet_ui_host_fs_rights_read() {
  return { bits: 1 };
}

function jet_ui_host_fs_rights_write() {
  return { bits: 2 };
}

function jet_ui_host_fs_rights_read_write() {
  return { bits: 3 };
}

function jet_ui_host_fs_grant(root, rights) {
  try {
    return jet_ui_web_ok({
      root: jet_ui_web_normalize_path(root),
      rights: { bits: jet_ui_web_rights_bits(rights) },
    });
  } catch (error) {
    return jet_ui_web_err(error?.tag != null ? error : jet_ui_web_error("InvalidRequest", String(error?.message ?? error)));
  }
}

function jet_ui_host_open_request(grant) {
  return {
    kind: jet_ui_web_enum("Open"),
    title: "Open File",
    grant,
    initial_directory: jet_ui_web_none(),
    filters: [],
    allow_multiple: false,
  };
}

function jet_ui_host_save_request(grant) {
  return {
    kind: jet_ui_web_enum("Save"),
    title: "Save File",
    grant,
    initial_directory: jet_ui_web_none(),
    filters: [],
    allow_multiple: false,
  };
}

function jet_ui_web_validate_request(request, expectedKind) {
  const kind = request?.kind?.tag ?? request?.kind;
  if (kind !== expectedKind) return jet_ui_web_err(jet_ui_web_error("InvalidRequest", `${expectedKind === "Open" ? "open_file" : "save_file"} requires a ${expectedKind.toLowerCase()} dialog request`));
  const rights = jet_ui_web_rights_bits(request?.grant?.rights);
  const needed = expectedKind === "Open" ? 1 : 2;
  if ((rights & needed) === 0) return jet_ui_web_err(jet_ui_web_error("ResourceDenied", `dialog ${expectedKind} requires ${expectedKind === "Open" ? "Read" : "Write"} access`));
  if (expectedKind === "Save" && request.allow_multiple) return jet_ui_web_err(jet_ui_web_error("InvalidRequest", "save dialogs cannot select multiple files"));
  const initial = jet_ui_web_option_value(request?.initial_directory);
  if (initial != null && (initial.grant_root !== request.grant.root || jet_ui_web_access_tag(initial.access) !== (expectedKind === "Open" ? "Read" : "Write"))) {
    return jet_ui_web_err(jet_ui_web_error("ResourceDenied", "initial directory is outside the dialog grant"));
  }
  return null;
}

function jet_ui_web_validate_selection(request, selection) {
  const files = Array.isArray(selection?.files) ? selection.files : [];
  if (files.length === 0) return jet_ui_web_err(jet_ui_web_error("InvalidRequest", "a completed file dialog must select at least one file"));
  if (!request.allow_multiple && files.length > 1) return jet_ui_web_err(jet_ui_web_error("InvalidRequest", "a single-selection dialog returned multiple files"));
  const access = request.kind?.tag === "Open" ? jet_ui_web_enum("Read") : jet_ui_web_enum("Write");
  for (const file of files) {
    const scoped = jet_ui_web_grant_scope(request.grant, file.path, access);
    if (scoped.tag !== "Ok") return scoped;
    const checked = scoped.values[0];
    if (file.path !== checked.path || file.grant_root !== checked.grant_root || jet_ui_web_access_tag(file.access) !== jet_ui_web_access_tag(checked.access)) {
      return jet_ui_web_err(jet_ui_web_error("ResourceDenied", `file dialog selection \`${file.path}\` is outside its grant`));
    }
  }
  return null;
}

function jet_ui_web_file_dialog(request, expectedKind) {
  const invalid = jet_ui_web_validate_request(request, expectedKind);
  if (invalid) return invalid;
  const requirement = jet_ui_web_require("FileDialog");
  if (requirement) return requirement;
  if (jet_ui_web_headless()) return jet_ui_web_cancelled("Headless");
  const picker = globalThis.__jetUiFileDialog;
  if (typeof picker !== "function") return jet_ui_web_err(jet_ui_web_error("CapabilityUnavailable", "UI.FileDialog"));
  const selection = picker(request);
  if (selection != null && typeof selection.then === "function") {
    throw new Error("UI.FileDialog Web adapter requires a synchronous scoped picker");
  }
  if (selection == null) return jet_ui_web_cancelled("User");
  const invalidSelection = jet_ui_web_validate_selection(request, selection);
  if (invalidSelection) return invalidSelection;
  return jet_ui_web_ok(selection);
}

function jet_ui_host_open_file(request) {
  return jet_ui_web_file_dialog(request, "Open");
}

function jet_ui_host_save_file(request) {
  return jet_ui_web_file_dialog(request, "Save");
}

function jet_ui_host_shortcut(key, modifiers) {
  const normalized = jet_ui_web_text(key).trim().toLowerCase();
  if (normalized.length === 0) return jet_ui_web_err(jet_ui_web_error("InvalidRequest", "shortcut key must not be empty"));
  const bits = Number(modifiers?.bits ?? modifiers ?? 0);
  return jet_ui_web_ok({ key: normalized, modifiers: { bits: (Number.isFinite(bits) ? bits : 0) & 0x1f } });
}

function jet_ui_shortcut_cmd(key) {
  const normalized = jet_ui_web_text(key).trim().toLowerCase();
  if (normalized.length === 0) throw new Error("checked .cmd literal must be valid");
  return { key: normalized, modifiers: { bits: 16 } };
}

function jet_ui_host_accessibility(name, description) {
  const accessibleName = jet_ui_web_text(name);
  const accessibleDescription = jet_ui_web_text(description);
  if (accessibleName.length === 0) {
    return jet_ui_web_err(jet_ui_web_error("InvalidRequest", "accessibility name must not be empty"));
  }
  return jet_ui_web_ok({
    name: jet_ui_web_some(accessibleName),
    description: accessibleDescription.length === 0 ? jet_ui_web_none() : jet_ui_web_some(accessibleDescription),
    states: [],
  });
}

function jet_ui_host_clipboard_read_text() {
  const requirement = jet_ui_web_require("Clipboard");
  if (requirement) return requirement;
  const state = jet_ui_web_host_state();
  if (!jet_ui_web_headless()) {
    const adapter = globalThis.__jetUiClipboard;
    const value = adapter.readText();
    if (value != null && typeof value.then === "function") throw new Error("Web Clipboard adapter must provide synchronous readText");
    state.clipboard = jet_ui_web_text(value);
  }
  return jet_ui_web_ok({ text: state.clipboard, selection: jet_ui_web_none() });
}

function jet_ui_host_clipboard_write_text(text) {
  const requirement = jet_ui_web_require("Clipboard");
  if (requirement) return requirement;
  const value = jet_ui_web_text(text);
  const state = jet_ui_web_host_state();
  if (jet_ui_web_headless()) {
    state.clipboard = value;
  } else {
    const result = globalThis.__jetUiClipboard.writeText(value);
    if (result != null && typeof result.then === "function") throw new Error("Web Clipboard adapter must provide synchronous writeText");
    state.clipboard = value;
  }
  return jet_ui_web_ok({ characters: jet_ui_web_scalar_count(value) });
}

function jet_ui_web_push_queue(queue, value, service) {
  if (queue.length >= JET_UI_WEB_MAX_QUEUED_EVENTS) return jet_ui_web_err(jet_ui_web_error("QueueFull", service));
  queue.push(value);
  return jet_ui_web_ok(null);
}

function jet_ui_web_ime_event(value) {
  const target = typeof value?.target === "object" ? value.target : jet_ui_node_id(value?.target ?? "");
  const phase = value?.phase?.tag != null ? value.phase : jet_ui_web_enum(String(value?.phase ?? "Commit"));
  const encodedComposition = value?.composition;
  const inputComposition = encodedComposition?.tag === "Err"
    ? undefined
    : jet_ui_web_option_value(encodedComposition) ?? encodedComposition;
  const hasComposition = phase.tag !== "Cancel" && (inputComposition !== undefined || value?.text !== undefined);
  const composition = hasComposition
    ? jet_ui_web_some({
      text: jet_ui_web_text(inputComposition?.text ?? value.text),
      selection: inputComposition?.selection ?? jet_ui_text_range(value.start ?? 0, value.end ?? value.start ?? 0),
      marked: inputComposition?.marked ?? jet_ui_web_none(),
    })
    : jet_ui_web_none();
  return { target, phase, composition };
}

function jet_ui_web_ime_queue(value, phase, text, start, end) {
  const event = value != null && typeof value === "object" && !Array.isArray(value)
    ? jet_ui_web_ime_event(value)
    : jet_ui_web_ime_event({ target: value, phase, text, start, end });
  return jet_ui_web_push_queue(jet_ui_web_host_state().ime, event, "IME");
}

function jet_ui_host_ime_poll() {
  const requirement = jet_ui_web_require("Ime");
  if (requirement) return requirement;
  const event = jet_ui_web_host_state().ime.shift();
  return jet_ui_web_ok(jet_ui_web_option(event));
}

function jet_ui_web_drop_item(value) {
  if (value?.tag != null) return value;
  if (value?.uri != null) return jet_ui_web_enum("Uri", jet_ui_web_text(value.uri));
  return jet_ui_web_enum("Text", jet_ui_web_text(value?.text ?? value));
}

function jet_ui_web_drag_event(value, phase, operation, items) {
  const target = typeof value?.target === "object" ? value.target : jet_ui_node_id(value?.target ?? "");
  const phaseValue = value?.phase?.tag != null ? value.phase : jet_ui_web_enum(String(value?.phase ?? phase ?? "Drop"));
  const operationValue = value?.operation?.tag != null ? value.operation : jet_ui_web_enum(String(value?.operation ?? operation ?? "Copy"));
  const itemValues = Array.from(value?.items ?? items ?? []).map(jet_ui_web_drop_item);
  return { target, phase: phaseValue, operation: operationValue, items: itemValues };
}

function jet_ui_web_drag_queue(value, phase, operation, items) {
  const event = value != null && typeof value === "object" && !Array.isArray(value)
    ? jet_ui_web_drag_event(value)
    : jet_ui_web_drag_event({ target: value, phase, operation, items });
  return jet_ui_web_push_queue(jet_ui_web_host_state().drag, event, "drag/drop");
}

function jet_ui_host_drag_poll() {
  const requirement = jet_ui_web_require("DragDrop");
  if (requirement) return requirement;
  const event = jet_ui_web_host_state().drag.shift();
  return jet_ui_web_ok(jet_ui_web_option(event));
}

function jet_ui_web_shortcut_id(shortcut) {
  return `${jet_ui_web_text(shortcut?.key).trim().toLowerCase()}\u0000${Number(shortcut?.modifiers?.bits ?? 0) & 0x1f}`;
}

function jet_ui_web_shortcut_candidates(event) {
  const key = jet_ui_web_text(event?.key || event?.code).trim().toLowerCase();
  const physical = (event?.ctrlKey ? 1 : 0)
    | (event?.altKey ? 2 : 0)
    | (event?.shiftKey ? 4 : 0)
    | (event?.metaKey ? 8 : 0);
  const platform = globalThis.navigator?.userAgentData?.platform ?? globalThis.navigator?.platform ?? "";
  const logical = physical | (String(platform).toLowerCase().includes("mac")
    ? (event?.metaKey ? 16 : 0)
    : (event?.ctrlKey ? 16 : 0));
  const values = [{ key, modifiers: { bits: logical } }];
  if (physical !== logical) values.push({ key, modifiers: { bits: physical } });
  return values;
}

function jet_ui_web_install_shortcut_listener() {
  const state = jet_ui_web_host_state();
  if (state.shortcutListener || typeof document === "undefined") return;
  state.shortcutListener = true;
  document.addEventListener("keydown", (event) => {
    for (const shortcut of jet_ui_web_shortcut_candidates(event)) {
      const binding = state.shortcuts.get(jet_ui_web_shortcut_id(shortcut));
      if (binding == null) continue;
      event.preventDefault?.();
      if (typeof globalThis.__jetUiShortcutDispatch === "function") {
        globalThis.__jetUiShortcutDispatch(binding, event);
      }
      break;
    }
  });
}

function jet_ui_host_shortcut_binding(shortcut, action) {
  const name = jet_ui_web_text(action);
  if (name.length === 0) return jet_ui_web_err(jet_ui_web_error("InvalidRequest", "shortcut action must not be empty"));
  return jet_ui_web_ok({ shortcut, action: name, node: jet_ui_web_none() });
}

function jet_ui_host_shortcuts_register(binding) {
  const requirement = jet_ui_web_require("Shortcuts");
  if (requirement) return requirement;
  const state = jet_ui_web_host_state();
  const id = jet_ui_web_shortcut_id(binding?.shortcut);
  const existing = state.shortcuts.get(id);
  if (existing != null) {
    return jet_ui_web_err(jet_ui_web_error("ShortcutConflict", binding.shortcut, existing.action));
  }
  const normalized = {
    shortcut: binding.shortcut,
    action: jet_ui_web_text(binding?.action),
    node: binding?.node ?? jet_ui_web_none(),
  };
  if (normalized.action.length === 0) return jet_ui_web_err(jet_ui_web_error("InvalidRequest", "shortcut action must not be empty"));
  state.shortcuts.set(id, normalized);
  jet_ui_web_install_shortcut_listener();
  return jet_ui_web_ok(normalized);
}

function jet_ui_host_shortcuts_dispatch(shortcut) {
  const requirement = jet_ui_web_require("Shortcuts");
  if (requirement) return requirement;
  const binding = jet_ui_web_host_state().shortcuts.get(jet_ui_web_shortcut_id(shortcut));
  return jet_ui_web_ok(binding == null
    ? jet_ui_web_enum("Unhandled")
    : jet_ui_web_enum("Dispatched", binding));
}

function jet_ui_host_attach_accessibility(node, accessibility) {
  const requirement = jet_ui_web_require("Accessibility");
  if (requirement) return requirement;
  return jet_ui_web_ok(jet_ui_node_accessibility(node, accessibility));
}

function jet_ui_host_project_accessibility(node, nodeId) {
  const requirement = jet_ui_web_require("Accessibility");
  if (requirement) return requirement;
  const metadata = jet_ui_web_option_value(node?.accessibility);
  const projection = metadata == null
    ? jet_ui_web_none()
    : jet_ui_web_some({ node: nodeId, metadata });
  return jet_ui_web_ok(projection);
}

// Source ABI for the generated HarfBuzz Wasm asset. HostServices.rs owns
// shaping and font selection; this module only adapts opaque handles and
// copies bytes between the app and HarfBuzz memories.
const JET_UI_HARFBUZZ_WASM_ARTIFACT = "wasm/jet_harfbuzz.wasm";
const JET_UI_HARFBUZZ_ABI_VERSION = 2;
const JET_UI_HARFBUZZ_MAX_TEXT_BYTES = 4 * 1024 * 1024;
const JET_UI_HARFBUZZ_MAX_FAMILY_BYTES = 1024;
const JET_UI_HARFBUZZ_MAX_GLYPHS = 1024 * 1024;
const JET_UI_HARFBUZZ_INFO_BYTES = 20;
const JET_UI_HARFBUZZ_POSITION_BYTES = 20;
const JET_UI_WASM_GLYPH_BYTES = 48;

const JET_UI_HARFBUZZ_EXPORTS = [
  "jet_hb_abi_version",
  "jet_hb_alloc",
  "jet_hb_free",
  "jet_hb_blob_create",
  "jet_hb_blob_destroy",
  "jet_hb_face_create",
  "jet_hb_face_get_glyph_count",
  "jet_hb_face_destroy",
  "jet_hb_font_create",
  "jet_hb_font_destroy",
  "jet_hb_font_set_scale",
  "jet_hb_ot_font_set_funcs",
  "jet_hb_buffer_create",
  "jet_hb_buffer_destroy",
  "jet_hb_buffer_add_utf8",
  "jet_hb_buffer_guess_segment_properties",
  "jet_hb_shape",
  "jet_hb_buffer_get_length",
  "jet_hb_buffer_get_glyph_infos",
  "jet_hb_buffer_get_glyph_positions",
];

function jet_ui_web_load_harfbuzz(source) {
  const exports = source?.exports ?? source;
  if (exports == null
      || exports.memory == null
      || exports.memory.buffer == null
      || JET_UI_HARFBUZZ_EXPORTS.some((name) => typeof exports[name] !== "function")) {
    throw new Error("UI.FontShaping HarfBuzz Wasm module has no low-level jet_hb ABI");
  }
  if (Number(exports.jet_hb_abi_version()) !== JET_UI_HARFBUZZ_ABI_VERSION) {
    throw new Error("UI.FontShaping HarfBuzz Wasm ABI version is unsupported");
  }
  jet_ui_web_host_state().harfbuzz = exports;
  return exports;
}

function jet_ui_web_harfbuzz_bytes(source) {
  const bytes = source?.bytes ?? source;
  if (bytes instanceof Uint8Array) return bytes;
  if (bytes instanceof ArrayBuffer
      || (typeof ArrayBuffer !== "undefined" && ArrayBuffer.isView(bytes))) {
    return new Uint8Array(bytes.buffer ?? bytes, bytes.byteOffset ?? 0, bytes.byteLength);
  }
  throw new Error(`UI.FontShaping HarfBuzz Wasm artifact is not bundled: ${JET_UI_HARFBUZZ_WASM_ARTIFACT}`);
}

// Emscripten's standalone reactor uses these standard WASI calls for its
// runtime scaffolding. The bridge performs no file or environment I/O; these
// implementations keep that unused scaffolding explicit and bounded.
function jet_ui_web_harfbuzz_runtime_host() {
  let memory = null;
  const writeU32 = (pointer, value) => {
    if (memory?.buffer == null) return;
    const offset = Number(pointer) >>> 0;
    if (offset > memory.buffer.byteLength - 4) return;
    new DataView(memory.buffer).setUint32(offset, value >>> 0, true);
  };
  return {
    imports: {
      env: {
        emscripten_notify_memory_growth() {},
      },
      wasi_snapshot_preview1: {
        fd_close() { return 52; },
        fd_write(_fd, _iovs, _iovsLength, nwritten) {
          writeU32(nwritten, 0);
          return 52;
        },
        environ_sizes_get(count, size) {
          writeU32(count, 0);
          writeU32(size, 0);
          return 0;
        },
        environ_get() { return 0; },
        fd_seek() { return 52; },
      },
    },
    attach(instance) {
      memory = instance?.exports?.memory ?? null;
    },
  };
}

function jet_ui_web_harfbuzz_finish_instance(instance, runtime) {
  runtime.attach(instance);
  const initialize = instance?.exports?._initialize
    ?? instance?.exports?.__initialize;
  if (typeof initialize === "function") initialize();
  return jet_ui_web_load_harfbuzz(instance);
}

async function jet_ui_web_load_harfbuzz_wasm(source = globalThis.__jetCanonicalHarfBuzzWasm) {
  if (source == null) {
    throw new Error(`UI.FontShaping HarfBuzz Wasm artifact is not bundled: ${JET_UI_HARFBUZZ_WASM_ARTIFACT}`);
  }
  if (source instanceof WebAssembly.Instance || source?.exports?.jet_hb_abi_version) {
    return jet_ui_web_load_harfbuzz(source);
  }
  const runtime = jet_ui_web_harfbuzz_runtime_host();
  if (source instanceof WebAssembly.Module) {
    return jet_ui_web_harfbuzz_finish_instance(
      new WebAssembly.Instance(source, runtime.imports),
      runtime,
    );
  }
  let bytes = source;
  if (typeof source === "string") {
    const response = await fetch(source);
    if (!response.ok) throw new Error(`UI.FontShaping HarfBuzz Wasm load failed (${response.status})`);
    bytes = await response.arrayBuffer();
  } else if (typeof Response !== "undefined" && source instanceof Response) {
    bytes = await source.arrayBuffer();
  } else {
    bytes = jet_ui_web_harfbuzz_bytes(source);
  }
  const result = await WebAssembly.instantiate(bytes, runtime.imports);
  return jet_ui_web_harfbuzz_finish_instance(result.instance ?? result, runtime);
}

function jet_ui_web_font_host_error(message) {
  return new Error(`UI.FontShaping ${message}`);
}

function jet_ui_web_font_host_u32(value, label) {
  const number = Number(value);
  if (!Number.isInteger(number) || number < 0 || number > 0xffffffff) {
    throw jet_ui_web_font_host_error(`${label} is outside the Wasm address range`);
  }
  return number >>> 0;
}

function jet_ui_web_font_host_memory(app) {
  if (app == null) {
    throw jet_ui_web_font_host_error("app Wasm imports were called before attach");
  }
  if (app.memory?.buffer == null) {
    throw jet_ui_web_font_host_error("app Wasm exports have no linear memory");
  }
  return app.memory;
}

function jet_ui_web_font_host_range(memory, pointer, length, label) {
  const offset = jet_ui_web_font_host_u32(pointer, `${label} pointer`);
  const size = jet_ui_web_font_host_u32(length, `${label} length`);
  if (offset > memory.buffer.byteLength || size > memory.buffer.byteLength - offset) {
    throw jet_ui_web_font_host_error(`${label} range is outside app Wasm memory`);
  }
  return { offset, size };
}

function jet_ui_web_font_host_hb_range(harfbuzz, pointer, length, label) {
  const offset = jet_ui_web_font_host_u32(pointer, `${label} pointer`);
  const size = jet_ui_web_font_host_u32(length, `${label} length`);
  if (offset > harfbuzz.memory.buffer.byteLength
      || size > harfbuzz.memory.buffer.byteLength - offset) {
    throw jet_ui_web_font_host_error(`${label} range is outside HarfBuzz Wasm memory`);
  }
  return { offset, size };
}

function jet_ui_web_font_host_copy_to_harfbuzz(app, harfbuzz, pointer, length, label, body) {
  const appRange = jet_ui_web_font_host_range(
    jet_ui_web_font_host_memory(app),
    pointer,
    length,
    label,
  );
  const size = Math.max(1, appRange.size);
  const copiedPointer = jet_ui_web_font_host_u32(
    harfbuzz.jet_hb_alloc(size),
    `${label} HarfBuzz allocation`,
  );
  if (copiedPointer === 0) {
    throw jet_ui_web_font_host_error(`${label} HarfBuzz allocation failed`);
  }
  try {
    const hbRange = jet_ui_web_font_host_hb_range(harfbuzz, copiedPointer, appRange.size, label);
    const appMemory = jet_ui_web_font_host_memory(app);
    new Uint8Array(harfbuzz.memory.buffer, hbRange.offset, hbRange.size).set(
      new Uint8Array(appMemory.buffer, appRange.offset, appRange.size),
    );
    return body(copiedPointer, appRange.size);
  } finally {
    harfbuzz.jet_hb_free(copiedPointer);
  }
}

function jet_ui_web_font_host_copy_glyph_array(
  app,
  harfbuzz,
  buffer,
  lengthPointer,
  getter,
  allocatorName,
  recordBytes,
) {
  const appMemory = jet_ui_web_font_host_memory(app);
  const lengthRange = jet_ui_web_font_host_range(appMemory, lengthPointer, 4, "glyph length");
  const hbLengthPointer = jet_ui_web_font_host_u32(
    harfbuzz.jet_hb_alloc(4),
    "glyph length HarfBuzz allocation",
  );
  if (hbLengthPointer === 0) {
    throw jet_ui_web_font_host_error("glyph length HarfBuzz allocation failed");
  }
  try {
    const hbResultPointer = jet_ui_web_font_host_u32(
      getter(jet_ui_web_font_host_u32(buffer, "buffer handle"), hbLengthPointer),
      "glyph array pointer",
    );
    const hbLength = new DataView(harfbuzz.memory.buffer).getUint32(hbLengthPointer, true);
    if (hbLength > JET_UI_HARFBUZZ_MAX_GLYPHS) {
      throw jet_ui_web_font_host_error("HarfBuzz returned too many glyphs");
    }
    const byteLength = hbLength * recordBytes;
    if (hbLength !== 0 && hbResultPointer === 0) {
      throw jet_ui_web_font_host_error("HarfBuzz returned a null glyph array");
    }
    if (hbLength !== 0) {
      jet_ui_web_font_host_hb_range(harfbuzz, hbResultPointer, byteLength, "glyph array");
    }
    const appPointer = hbLength === 0
      ? 0
      : jet_ui_web_font_host_u32(
        app[allocatorName](hbLength),
        "app glyph scratch allocation",
      );
    if (hbLength !== 0 && appPointer === 0) {
      throw jet_ui_web_font_host_error("app glyph scratch allocation failed");
    }
    if (hbLength !== 0) {
      const destination = jet_ui_web_font_host_range(appMemory, appPointer, byteLength, "glyph scratch");
      new Uint8Array(appMemory.buffer, destination.offset, destination.size).set(
        new Uint8Array(harfbuzz.memory.buffer, hbResultPointer, byteLength),
      );
    }
    new DataView(appMemory.buffer).setUint32(lengthRange.offset, hbLength, true);
    return appPointer;
  } finally {
    harfbuzz.jet_hb_free(hbLengthPointer);
  }
}

// Return the imports expected by HostServices.rs and attach the app memory
// immediately after jetDom.instantiateWasm returns. No shaping path can run
// before attach: every import checks this invariant.
let jet_ui_web_harfbuzz_app = null;
let jet_ui_web_harfbuzz_import_object = null;

async function jet_ui_web_harfbuzz_imports() {
  const harfbuzz = await jet_ui_web_load_harfbuzz_wasm();
  const attached = () => {
    if (jet_ui_web_harfbuzz_app == null) {
      throw jet_ui_web_font_host_error("app Wasm imports were called before attach");
    }
    jet_ui_web_font_host_memory(jet_ui_web_harfbuzz_app);
    return jet_ui_web_harfbuzz_app;
  };
  const imports = {
    jet_hb_blob_create(dataPointer, length, memoryMode, userData, destroy) {
      const owner = attached();
      return jet_ui_web_font_host_copy_to_harfbuzz(
        owner,
        harfbuzz,
        dataPointer,
        length,
        "font blob",
        (copiedPointer, copiedLength) => harfbuzz.jet_hb_blob_create(
          copiedPointer,
          copiedLength,
          jet_ui_web_font_host_u32(memoryMode, "blob memory mode"),
          0,
          0,
        ),
      );
    },
    jet_hb_blob_destroy(blob) {
      attached();
      harfbuzz.jet_hb_blob_destroy(jet_ui_web_font_host_u32(blob, "blob handle"));
    },
    jet_hb_face_create(blob, index) {
      attached();
      return harfbuzz.jet_hb_face_create(
        jet_ui_web_font_host_u32(blob, "blob handle"),
        jet_ui_web_font_host_u32(index, "face index"),
      );
    },
    jet_hb_face_get_glyph_count(face) {
      attached();
      return harfbuzz.jet_hb_face_get_glyph_count(
        jet_ui_web_font_host_u32(face, "face handle"),
      );
    },
    jet_hb_face_destroy(face) {
      attached();
      harfbuzz.jet_hb_face_destroy(jet_ui_web_font_host_u32(face, "face handle"));
    },
    jet_hb_font_create(face) {
      attached();
      return harfbuzz.jet_hb_font_create(jet_ui_web_font_host_u32(face, "face handle"));
    },
    jet_hb_font_destroy(font) {
      attached();
      harfbuzz.jet_hb_font_destroy(jet_ui_web_font_host_u32(font, "font handle"));
    },
    jet_hb_font_set_scale(font, xScale, yScale) {
      attached();
      harfbuzz.jet_hb_font_set_scale(
        jet_ui_web_font_host_u32(font, "font handle"),
        Number(xScale) | 0,
        Number(yScale) | 0,
      );
    },
    jet_hb_ot_font_set_funcs(font) {
      attached();
      harfbuzz.jet_hb_ot_font_set_funcs(jet_ui_web_font_host_u32(font, "font handle"));
    },
    jet_hb_buffer_create() {
      attached();
      return harfbuzz.jet_hb_buffer_create();
    },
    jet_hb_buffer_destroy(buffer) {
      attached();
      harfbuzz.jet_hb_buffer_destroy(jet_ui_web_font_host_u32(buffer, "buffer handle"));
    },
    jet_hb_buffer_add_utf8(buffer, textPointer, textLength, itemOffset, itemLength) {
      const owner = attached();
      const length = Number(textLength);
      if (!Number.isInteger(length) || length < 0 || length > JET_UI_HARFBUZZ_MAX_TEXT_BYTES) {
        throw jet_ui_web_font_host_error("text length is invalid");
      }
      if (length === 0) {
        harfbuzz.jet_hb_buffer_add_utf8(
          jet_ui_web_font_host_u32(buffer, "buffer handle"),
          0,
          0,
          jet_ui_web_font_host_u32(itemOffset, "text item offset"),
          Number(itemLength) | 0,
        );
        return;
      }
      jet_ui_web_font_host_copy_to_harfbuzz(
        owner,
        harfbuzz,
        textPointer,
        length,
        "text",
        (copiedPointer, copiedLength) => harfbuzz.jet_hb_buffer_add_utf8(
          jet_ui_web_font_host_u32(buffer, "buffer handle"),
          copiedPointer,
          copiedLength,
          jet_ui_web_font_host_u32(itemOffset, "text item offset"),
          Number(itemLength) | 0,
        ),
      );
    },
    jet_hb_buffer_guess_segment_properties(buffer) {
      attached();
      harfbuzz.jet_hb_buffer_guess_segment_properties(
        jet_ui_web_font_host_u32(buffer, "buffer handle"),
      );
    },
    jet_hb_shape(font, buffer, features, featureCount) {
      attached();
      harfbuzz.jet_hb_shape(
        jet_ui_web_font_host_u32(font, "font handle"),
        jet_ui_web_font_host_u32(buffer, "buffer handle"),
        jet_ui_web_font_host_u32(features, "feature pointer"),
        jet_ui_web_font_host_u32(featureCount, "feature count"),
      );
    },
    jet_hb_buffer_get_length(buffer) {
      attached();
      return harfbuzz.jet_hb_buffer_get_length(
        jet_ui_web_font_host_u32(buffer, "buffer handle"),
      );
    },
    jet_hb_buffer_get_glyph_infos(buffer, lengthPointer) {
      const owner = attached();
      return jet_ui_web_font_host_copy_glyph_array(
        owner,
        harfbuzz,
        buffer,
        lengthPointer,
        harfbuzz.jet_hb_buffer_get_glyph_infos,
        "jet_hb_wasm_info_alloc",
        JET_UI_HARFBUZZ_INFO_BYTES,
      );
    },
    jet_hb_buffer_get_glyph_positions(buffer, lengthPointer) {
      const owner = attached();
      return jet_ui_web_font_host_copy_glyph_array(
        owner,
        harfbuzz,
        buffer,
        lengthPointer,
        harfbuzz.jet_hb_buffer_get_glyph_positions,
        "jet_hb_wasm_positions_alloc",
        JET_UI_HARFBUZZ_POSITION_BYTES,
      );
    },
  };
  jet_ui_web_harfbuzz_import_object = Object.freeze(imports);
  return jet_ui_web_harfbuzz_import_object;
}

function jet_ui_web_harfbuzz_bind_app(wasmExports) {
  if (wasmExports == null
      || typeof wasmExports.jet_font_shape_wasm_text_alloc !== "function"
      || typeof wasmExports.jet_font_shape_wasm_family_alloc !== "function"
      || typeof wasmExports.jet_font_shape_wasm_shape !== "function"
      || typeof wasmExports.jet_hb_wasm_info_alloc !== "function"
      || typeof wasmExports.jet_hb_wasm_positions_alloc !== "function"
      || typeof wasmExports.jet_font_shape_wasm_output_ptr !== "function"
      || typeof wasmExports.jet_font_shape_wasm_output_len !== "function"
      || typeof wasmExports.jet_font_shape_wasm_output_advance_x !== "function"
      || typeof wasmExports.jet_font_shape_wasm_output_advance_y !== "function"
      || typeof wasmExports.jet_font_shape_wasm_error_ptr !== "function"
      || typeof wasmExports.jet_font_shape_wasm_error_len !== "function") {
    throw jet_ui_web_font_host_error("app Wasm exports have no font-shape value ABI");
  }
  jet_ui_web_font_host_memory(wasmExports);
  jet_ui_web_harfbuzz_app = wasmExports;
}

function jet_ui_web_font_style_code(style) {
  const tag = style?.tag ?? style ?? "Body";
  if (tag === "Body") return 0;
  if (tag === "Title") return 1;
  if (tag === "Monospace") return 2;
  throw jet_ui_web_font_host_error("font style is invalid");
}

function jet_ui_web_app_wasm() {
  try {
    if (__jetPreludeWasm == null) {
      throw jet_ui_web_font_host_error("app Wasm is unavailable");
    }
    return __jetPreludeWasm;
  } catch (error) {
    if (error instanceof ReferenceError) {
      throw jet_ui_web_font_host_error("app Wasm is not initialized");
    }
    throw error;
  }
}

function jet_ui_web_read_app_bytes(wasm, pointer, length, label) {
  const range = jet_ui_web_font_host_range(
    jet_ui_web_font_host_memory(wasm),
    pointer,
    length,
    label,
  );
  return new Uint8Array(wasm.memory.buffer, range.offset, range.size);
}

function jet_font_shape(text, face) {
  const wasm = jet_ui_web_app_wasm();
  const textBytes = new TextEncoder().encode(jet_ui_web_text(text));
  if (textBytes.length > JET_UI_HARFBUZZ_MAX_TEXT_BYTES) {
    throw jet_ui_web_font_host_error("text exceeds the supported input size");
  }
  const hasFace = face != null;
  const familyBytes = new TextEncoder().encode(hasFace ? jet_ui_web_text(face.family) : "");
  if (familyBytes.length > JET_UI_HARFBUZZ_MAX_FAMILY_BYTES) {
    throw jet_ui_web_font_host_error("font family exceeds the supported input size");
  }
  const textPointer = wasm.jet_font_shape_wasm_text_alloc(textBytes.length);
  const familyPointer = wasm.jet_font_shape_wasm_family_alloc(familyBytes.length);
  if (textBytes.length !== 0 && Number(textPointer) === 0) {
    throw jet_ui_web_font_host_error("text scratch allocation failed");
  }
  if (familyBytes.length !== 0 && Number(familyPointer) === 0) {
    throw jet_ui_web_font_host_error("font family scratch allocation failed");
  }
  if (textBytes.length !== 0) {
    new Uint8Array(wasm.memory.buffer).set(textBytes, Number(textPointer) >>> 0);
  }
  if (familyBytes.length !== 0) {
    new Uint8Array(wasm.memory.buffer).set(familyBytes, Number(familyPointer) >>> 0);
  }
  const rawCount = Number(wasm.jet_font_shape_wasm_shape(
    Number(textPointer) >>> 0,
    textBytes.length,
    Number(familyPointer) >>> 0,
    familyBytes.length,
    hasFace ? Number(face.size) : 0,
    hasFace ? jet_ui_web_font_style_code(face.style) : 0,
  ));
  if (!Number.isSafeInteger(rawCount)) {
    throw jet_ui_web_font_host_error("font shape returned an invalid glyph count");
  }
  if (rawCount < 0) {
    const errorPointer = Number(wasm.jet_font_shape_wasm_error_ptr()) >>> 0;
    const errorLength = Number(wasm.jet_font_shape_wasm_error_len());
    const message = errorLength === 0
      ? "font shape failed"
      : new TextDecoder().decode(
        jet_ui_web_read_app_bytes(wasm, errorPointer, errorLength, "font shape error"),
      );
    throw new Error(message);
  }
  if (rawCount > JET_UI_HARFBUZZ_MAX_GLYPHS) {
    throw jet_ui_web_font_host_error("font shape returned too many glyphs");
  }
  const outputCount = Number(wasm.jet_font_shape_wasm_output_len());
  if (outputCount !== rawCount) {
    throw jet_ui_web_font_host_error("font shape output length changed unexpectedly");
  }
  const outputPointer = Number(wasm.jet_font_shape_wasm_output_ptr()) >>> 0;
  const outputBytes = rawCount * JET_UI_WASM_GLYPH_BYTES;
  const output = outputBytes === 0
    ? null
    : new DataView(
      wasm.memory.buffer,
      jet_ui_web_font_host_range(wasm.memory, outputPointer, outputBytes, "font shape output").offset,
      outputBytes,
    );
  const glyphs = [];
  for (let index = 0; index < rawCount; index += 1) {
    const offset = index * JET_UI_WASM_GLYPH_BYTES;
    glyphs.push({
      id: Number(output.getBigInt64(offset, true)),
      cluster: Number(output.getBigInt64(offset + 8, true)),
      x: output.getFloat64(offset + 16, true),
      y: output.getFloat64(offset + 24, true),
      advance_x: output.getFloat64(offset + 32, true),
      advance_y: output.getFloat64(offset + 40, true),
    });
  }
  const advanceX = Number(wasm.jet_font_shape_wasm_output_advance_x());
  const advanceY = Number(wasm.jet_font_shape_wasm_output_advance_y());
  if (!Number.isFinite(advanceX) || !Number.isFinite(advanceY)
      || glyphs.some((glyph) => ![
        glyph.id,
        glyph.cluster,
        glyph.x,
        glyph.y,
        glyph.advance_x,
        glyph.advance_y,
      ].every(Number.isFinite))) {
    throw jet_ui_web_font_host_error("font shape returned non-finite values");
  }
  return {
    glyphs,
    advance_x: advanceX,
    advance_y: advanceY,
    shaper: jet_ui_web_enum("HarfBuzz"),
    deterministic: true,
    approximate: false,
  };
}

// DOM event adapters call these through globalThis because DomRuntime.js is a
// separate ES module from the generated app prelude.
globalThis.jet_ui_web_ime_queue = jet_ui_web_ime_queue;
globalThis.jet_ui_web_drag_queue = jet_ui_web_drag_queue;
globalThis.jet_ui_web_harfbuzz_imports = jet_ui_web_harfbuzz_imports;
globalThis.jet_ui_web_harfbuzz_bind_app = jet_ui_web_harfbuzz_bind_app;

// D-SPACE-GEOMETRY1=A: the Web carrier mirrors the native coordinate kernel.
// Nominal spaces are compile-time facts; frame ids are the only dynamic
// identity retained in the value.
function jet_math_geometry_point(x, y, frame_id = 0) {
  return { x: Number(x), y: Number(y), frame_id: Number(frame_id) };
}
function jet_math_Point2_new(x, y, frame_id) { return jet_math_geometry_point(x, y, frame_id); }
function jet_math_Delta2_new(x, y, frame_id) { return jet_math_geometry_point(x, y, frame_id); }
function jet_math_ScreenPoint_new(x, y) { return jet_math_geometry_point(x, y); }
function jet_math_WorldPoint_new(x, y) { return jet_math_geometry_point(x, y); }
function jet_math_ViewPoint_new(x, y) { return jet_math_geometry_point(x, y); }
function jet_math_CameraPoint_new(x, y) { return jet_math_geometry_point(x, y); }
function jet_math_DevicePoint_new(x, y) { return jet_math_geometry_point(x, y); }
function jet_math_ScreenDelta_new(x, y) { return jet_math_geometry_point(x, y); }
function jet_math_WorldDelta_new(x, y) { return jet_math_geometry_point(x, y); }
function jet_math_ViewDelta_new(x, y) { return jet_math_geometry_point(x, y); }
function jet_math_CameraDelta_new(x, y) { return jet_math_geometry_point(x, y); }
function jet_math_DeviceDelta_new(x, y) { return jet_math_geometry_point(x, y); }
function jet_math_geometry_add(a, b) {
  return jet_math_geometry_point(a.x + b.x, a.y + b.y, a.frame_id);
}
function jet_math_geometry_sub(a, b) {
  return jet_math_geometry_point(a.x - b.x, a.y - b.y, a.frame_id);
}
function jet_math_geometry_checked(a, b, subtract) {
  if (a.frame_id !== 0 && b.frame_id !== 0 && a.frame_id !== b.frame_id) {
    return jet_ui_web_err("coordinate values belong to different frames");
  }
  const frame_id = a.frame_id !== 0 ? a.frame_id : b.frame_id;
  return jet_ui_web_ok(jet_math_geometry_point(
    subtract ? a.x - b.x : a.x + b.x,
    subtract ? a.y - b.y : a.y + b.y,
    frame_id,
  ));
}
function jet_math_ScreenPoint_add(a, b) { return jet_math_geometry_add(a, b); }
function jet_math_ScreenPoint_sub(a, b) { return jet_math_geometry_sub(a, b); }
function jet_math_WorldPoint_add(a, b) { return jet_math_geometry_add(a, b); }
function jet_math_WorldPoint_sub(a, b) { return jet_math_geometry_sub(a, b); }
function jet_math_ViewPoint_add(a, b) { return jet_math_geometry_add(a, b); }
function jet_math_ViewPoint_sub(a, b) { return jet_math_geometry_sub(a, b); }
function jet_math_CameraPoint_add(a, b) { return jet_math_geometry_add(a, b); }
function jet_math_CameraPoint_sub(a, b) { return jet_math_geometry_sub(a, b); }
function jet_math_DevicePoint_add(a, b) { return jet_math_geometry_add(a, b); }
function jet_math_DevicePoint_sub(a, b) { return jet_math_geometry_sub(a, b); }
function jet_math_ScreenDelta_add(a, b) { return jet_math_geometry_add(a, b); }
function jet_math_ScreenDelta_sub(a, b) { return jet_math_geometry_sub(a, b); }
function jet_math_WorldDelta_add(a, b) { return jet_math_geometry_add(a, b); }
function jet_math_WorldDelta_sub(a, b) { return jet_math_geometry_sub(a, b); }
function jet_math_ViewDelta_add(a, b) { return jet_math_geometry_add(a, b); }
function jet_math_ViewDelta_sub(a, b) { return jet_math_geometry_sub(a, b); }
function jet_math_CameraDelta_add(a, b) { return jet_math_geometry_add(a, b); }
function jet_math_CameraDelta_sub(a, b) { return jet_math_geometry_sub(a, b); }
function jet_math_DeviceDelta_add(a, b) { return jet_math_geometry_add(a, b); }
function jet_math_DeviceDelta_sub(a, b) { return jet_math_geometry_sub(a, b); }
function jet_math_Point2_add(a, b) {
  return jet_math_geometry_checked(a, b, false);
}
function jet_math_Point2_sub(a, b) {
  return jet_math_geometry_checked(a, b, true);
}
function jet_math_Delta2_add(a, b) {
  return jet_math_geometry_checked(a, b, false);
}
function jet_math_Delta2_sub(a, b) {
  return jet_math_geometry_checked(a, b, true);
}
function jet_math_Transform_new(m00, m01, m10, m11, tx, ty, from_frame = 0, to_frame = 0) {
  return { m00, m01, m10, m11, tx, ty, from_frame, to_frame };
}
function jet_math_Transform_affine(m00, m01, m10, m11, tx, ty, from_frame, to_frame) {
  return jet_math_Transform_new(m00, m01, m10, m11, tx, ty, from_frame, to_frame);
}
function jet_math_Transform2_affine(m00, m01, m10, m11, tx, ty, from_frame, to_frame) {
  return jet_math_Transform_affine(m00, m01, m10, m11, tx, ty, from_frame, to_frame);
}
function jet_math_Transform_then(first, next) {
  if (first.to_frame !== 0 && next.from_frame !== 0 && first.to_frame !== next.from_frame) {
    return jet_ui_web_err("transform composition has mismatched frame identity");
  }
  return jet_ui_web_ok(jet_math_Transform_new(
    next.m00 * first.m00 + next.m01 * first.m10,
    next.m00 * first.m01 + next.m01 * first.m11,
    next.m10 * first.m00 + next.m11 * first.m10,
    next.m10 * first.m01 + next.m11 * first.m11,
    next.m00 * first.tx + next.m01 * first.ty + next.tx,
    next.m10 * first.tx + next.m11 * first.ty + next.ty,
    first.from_frame,
    next.to_frame,
  ));
}
function jet_math_Transform_inverse(transform) {
  const det = transform.m00 * transform.m11 - transform.m01 * transform.m10;
  if (!Number.isFinite(det) || Math.abs(det) <= Number.EPSILON) {
    return jet_ui_web_err("transform is singular and has no inverse");
  }
  return jet_ui_web_ok(jet_math_Transform_new(
    transform.m11 / det, -transform.m01 / det,
    -transform.m10 / det, transform.m00 / det,
    (transform.m01 * transform.ty - transform.m11 * transform.tx) / det,
    (transform.m10 * transform.tx - transform.m00 * transform.ty) / det,
    transform.to_frame, transform.from_frame,
  ));
}
function jet_math_Transform_point(transform, point) {
  if (point.frame_id !== 0 && point.frame_id !== transform.from_frame) {
    return jet_ui_web_err("coordinate point belongs to a stale frame");
  }
  return jet_ui_web_ok(jet_math_geometry_point(
    transform.m00 * point.x + transform.m01 * point.y + transform.tx,
    transform.m10 * point.x + transform.m11 * point.y + transform.ty,
    transform.to_frame,
  ));
}
function jet_math_Transform_point_at_depth(transform, point, depth) {
  const numericDepth = Number(depth);
  if (!Number.isFinite(numericDepth)) return jet_ui_web_err("perspective depth must be finite");
  const origin = jet_math_Transform_point(transform, point);
  if (origin.tag !== "Ok") return origin;
  const value = origin.values[0];
  return jet_ui_web_ok(jet_math_geometry_point(
    value.x + transform.m00 * numericDepth,
    value.y + transform.m10 * numericDepth,
    value.frame_id,
  ));
}
function jet_math_Transform_ray(transform, point) {
  const origin = jet_math_Transform_point(transform, point);
  if (origin.tag !== "Ok") return origin;
  return jet_ui_web_ok({
    origin: origin.values[0],
    direction: jet_math_geometry_point(transform.m00, transform.m10, transform.to_frame),
  });
}
function jet_math_Transform2_then(first, next) { return jet_math_Transform_then(first, next); }
function jet_math_Transform2_inverse(transform) { return jet_math_Transform_inverse(transform); }
function jet_math_Transform2_point(transform, point) { return jet_math_Transform_point(transform, point); }
function jet_math_Transform2_point_at_depth(transform, point, depth) {
  return jet_math_Transform_point_at_depth(transform, point, depth);
}
function jet_math_Transform2_ray(transform, point) { return jet_math_Transform_ray(transform, point); }
