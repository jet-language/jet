  // The Debug and Profile views: every debugger panel at once (source, call
  // stack, locals, watches, tasks and the console) over the one debug session
  // the graph overlay uses, and the profile timeline of one .jettrace, which
  // the server reduces to the window shown. State here is `var` so the views
  // can render before this file's statements run.
  var profileState = { doc: null, trace: "", zoom: [], busy: false, selected: null, dragFrom: null, dragTo: null };
  var PROFILE_LANE_COLORS = { spans: "#60a5fa", zones: "#a78bfa", waits: "#f59e0b", io: "#34d399" };
  var PROFILE_LANE_HEIGHT = 44;
  var PROFILE_AXIS_HEIGHT = 22;
  var PROFILE_LABEL_WIDTH = 64;

  function debugViewElement(id) {
    return document.getElementById(id);
  }

  function debugViewRow(parts, className) {
    const row = document.createElement("div");
    row.className = "row" + (className ? " " + className : "");
    for (const part of parts) {
      const span = document.createElement("span");
      if (part[1]) span.className = part[1];
      span.textContent = part[0];
      row.appendChild(span);
    }
    return row;
  }

  function debugViewFill(id, rows, empty) {
    const host = debugViewElement(id);
    if (!host) return;
    host.innerHTML = "";
    if (!rows.length) {
      const note = document.createElement("div");
      note.className = "empty";
      note.textContent = empty;
      host.appendChild(note);
      return;
    }
    for (const row of rows) host.appendChild(row);
  }

  // The 1-based lines that carry a breakpoint at the current revision.
  function debugViewBreakLines(source) {
    const lines = new Set();
    if (!latestDoc) return lines;
    const encoder = new TextEncoder();
    const bytes = encoder.encode(source);
    for (const mark of (debugState.breakpoints || [])) {
      if (mark.revision !== latestDoc.revision || !mark.source_span) continue;
      let line = 1;
      const limit = Math.min(mark.source_span.start, bytes.length);
      for (let i = 0; i < limit; i++) if (bytes[i] === 10) line++;
      lines.add(line);
    }
    return lines;
  }

  // A line breakpoint, anchored to the line's byte span like a node's.
  function toggleLineBreakpoint(line) {
    if (!latestDoc || typeof latestDoc.source_text !== "string") return;
    loadDebugState(latestDoc);
    const encoder = new TextEncoder();
    const lines = latestDoc.source_text.split("\n");
    if (line < 1 || line > lines.length) return;
    let start = 0;
    for (let i = 0; i < line - 1; i++) start += encoder.encode(lines[i]).length + 1;
    const end = start + encoder.encode(lines[line - 1].replace(/\r$/, "")).length;
    const marked = debugViewBreakLines(latestDoc.source_text);
    if (marked.has(line)) {
      const bytes = encoder.encode(latestDoc.source_text);
      debugState.breakpoints = debugState.breakpoints.filter((mark) => {
        if (mark.revision !== latestDoc.revision || !mark.source_span) return true;
        let at = 1;
        for (let i = 0; i < Math.min(mark.source_span.start, bytes.length); i++) if (bytes[i] === 10) at++;
        return at !== line;
      });
    } else {
      if (debugState.breakpoints.length >= 128) {
        showToast("Breakpoint limit reached; source was kept", { isError: true });
        return;
      }
      const span = { start, end };
      debugState.breakpoints.push({ anchor: spanAnchor(span), source_span: span, revision: latestDoc.revision });
    }
    saveDebugState();
    renderDebugView();
    if (debugSessionId) runDebug([]);
  }

  function renderDebugView() {
    if (typeof viewMode === "undefined" || viewMode !== "debug") return;
    const overlay = debugOverlay;
    const state = debugViewElement("debug-view-state");
    if (state) {
      const line = overlay && overlay.active_line ? " · line " + overlay.active_line : "";
      const engine = debugSessionInfo ? " · " + debugEngineLabel(debugSessionInfo.tier) : "";
      state.textContent = (debugSessionId ? "Paused" : overlay ? "Finished" : "No session") + line + engine;
    }
    // Reverse steps and `why` run in a replay; a dump runs nothing.
    const tier = debugSessionId && debugSessionInfo ? debugSessionInfo.tier : "";
    for (const id of ["debug-view-reverse", "debug-view-reverse-step", "debug-view-reverse-finish", "debug-view-reverse-continue", "debug-view-why"]) {
      const control = debugViewElement(id);
      if (control) control.disabled = tier !== "native-recorded";
    }
    for (const id of ["debug-view-continue", "debug-view-next", "debug-view-step", "debug-view-finish"]) {
      const button = debugViewElement(id);
      if (button) button.disabled = !debugSessionId || tier === "native-dump";
    }
    const stop = debugViewElement("debug-view-stop");
    if (stop) stop.disabled = !debugSessionId;
    // Source: the whole entry from the graph document, or the overlay's
    // window around the stop.
    let firstLine = 1;
    let lines = [];
    if (latestDoc && typeof latestDoc.source_text === "string") {
      lines = latestDoc.source_text.split("\n");
      if (lines.length && lines[lines.length - 1] === "") lines.pop();
    } else if (overlay && overlay.source) {
      firstLine = overlay.source.first_line || 1;
      lines = overlay.source.lines || [];
    }
    const breaks = latestDoc && typeof latestDoc.source_text === "string" ? debugViewBreakLines(latestDoc.source_text) : new Set();
    const active = overlay && overlay.active_line;
    const sourceRows = lines.map((text, index) => {
      const number = firstLine + index;
      const classes = (number === active ? "is-active" : "") + (breaks.has(number) ? " is-break" : "");
      const row = debugViewRow([[(breaks.has(number) ? "● " : "") + number, "gutter"], [text, ""]], classes.trim());
      row.dataset.line = String(number);
      row.addEventListener("click", () => toggleLineBreakpoint(number));
      return row;
    });
    debugViewFill("debug-source-lines", sourceRows, "No source yet");
    const activeRow = document.querySelector("#debug-source-lines .is-active");
    if (activeRow && activeRow.scrollIntoView) activeRow.scrollIntoView({ block: "center" });
    const stack = (overlay && overlay.call_stack) || [];
    debugViewFill("debug-stack-rows", stack.map((frame) => debugViewRow([[frame, ""]])), "Not paused");
    const locals = (overlay && overlay.locals) || [];
    debugViewFill("debug-locals-rows", locals.map(debugLocalRow), "Not paused");
    // Each watch with the value the stop read, or why it has none: stale
    // when it was drawn on another revision, else out of the stop's scope.
    const watched = (overlay && overlay.watches) || [];
    const watchRows = (debugState.watches || []).map((watch) => {
      const stale = !!overlay && watch.revision !== overlay.revision;
      const found = watched.find((entry) => entry.name === watch.name && (entry.state === "stale") === stale);
      const value = !found ? "(not in scope)" : stale ? "(stale: source changed)" : found.value;
      const row = debugViewRow([[watch.name, "key"], [value, ""], [found && found.type || "", "type"]]);
      row.dataset.anchor = (watch.anchor ? watch.anchor + "@" : "") + watch.revision;
      return row;
    });
    debugViewFill("debug-watches-rows", watchRows, "No watches");
    const tasks = (overlay && overlay.tasks) || [];
    debugViewFill("debug-tasks-rows", tasks.map((task) => debugViewRow([[typeof task === "string" ? task : JSON.stringify(task), ""]])), "No task list from this session");
    const trace = (overlay && overlay.trace) || [];
    debugViewFill("debug-console-log", trace.map((line) => debugViewRow([[line, ""]])), "Start the entry to debug it");
    const log = debugViewElement("debug-console-log");
    if (log) log.scrollTop = log.scrollHeight;
    renderDebugMachine();
  }

  // The machine panels: Registers, Memory and Disassembly
  // beside the others. Each panel's header folds it, and the fold is kept in
  // local settings; a folded Registers or Disassembly panel is not sent.
  // Memory and more disassembly are a request's `reads`, which stay out of
  // the console. The page keeps at most DEBUG_MEMORY_PAGES pages read at the
  // stop, so Back and a followed pointer read once.
  var DEBUG_MEMORY_PAGE = 4096;
  var DEBUG_MEMORY_PAGES = 64;
  var DEBUG_MEMORY_ROW = 16;
  var DEBUG_LISTING_MORE = 16;
  var DEBUG_LISTING_KEPT = 256;
  var DEBUG_MEMORY_TYPES = { "": 1, U8: 1, I8: 1, U16: 2, I16: 2, U32: 4, I32: 4, F32: 4, U64: 8, I64: 8, Int: 8, Float: 8 };
  var debugMachine = { session: null, where: "", type: "", back: [], pages: [], listing: [], before: [], after: [] };

  function debugPanelFolded(id) {
    return storedFlag("jet.canvas.debug.folded." + id);
  }

  // The overlay fields of the folded panels, which the server leaves out.
  function debugFoldedPanels() {
    const folded = [];
    if (debugPanelFolded("debug-registers")) folded.push("registers");
    if (debugPanelFolded("debug-disassembly")) folded.push("disassembly");
    return folded;
  }

  // The reads a request carries: the Memory panel's page, unless it is
  // folded or read at this stop already, and the disassembly asked for. A
  // request that runs commands moves the stop, and another session has
  // its own, so what was read is dropped.
  function debugMachineReads(commands) {
    if (commands.length || debugMachine.session !== debugSessionId) {
      debugMachine.session = debugSessionId;
      debugMachine.pages = [];
      debugMachine.before = [];
      debugMachine.after = [];
    }
    const reads = debugMachine.listing.splice(0);
    if (debugMachine.where && !debugPanelFolded("debug-memory") && !debugMemoryPage()) {
      const size = DEBUG_MEMORY_TYPES[debugMachine.type] || 1;
      reads.push({ kind: "memory", where: debugMachine.where, type: debugMachine.type, count: DEBUG_MEMORY_PAGE / size });
    }
    return reads;
  }

  // Keep the answers to a request's reads.
  function debugMachineAnswer(reads) {
    for (const read of reads) {
      if (read.kind === "disassembly") {
        if (read.skip < 0) debugMachine.before = read.rows.concat(debugMachine.before).slice(0, DEBUG_LISTING_KEPT);
        else debugMachine.after = debugMachine.after.concat(read.rows).slice(0, DEBUG_LISTING_KEPT);
        continue;
      }
      const key = read.where + "|" + read.type;
      debugMachine.pages = debugMachine.pages.filter((page) => page.key !== key);
      debugMachine.pages.push({ key, read });
      if (debugMachine.pages.length > DEBUG_MEMORY_PAGES) debugMachine.pages.shift();
    }
  }

  function debugMemoryPage() {
    const key = debugMachine.where + "|" + debugMachine.type;
    const page = debugMachine.pages.find((known) => known.key === key);
    return page ? page.read : null;
  }

  // Show memory at `where` as `type` ("" for bytes); the place shown before
  // is kept for Back unless `remember` is false.
  function debugMemoryOpen(where, type, remember = true) {
    if (!where) return;
    if (remember && debugMachine.where && debugMachine.where !== where) {
      debugMachine.back.push({ where: debugMachine.where, type: debugMachine.type });
      if (debugMachine.back.length > DEBUG_MEMORY_PAGES) debugMachine.back.shift();
    }
    debugMachine.where = where;
    debugMachine.type = type in DEBUG_MEMORY_TYPES ? type : "";
    const input = debugViewElement("debug-memory-where");
    if (input) input.value = where;
    const select = debugViewElement("debug-memory-type");
    if (select) select.value = debugMachine.type;
    if (debugSessionId && !debugMemoryPage()) runDebug([]);
    else renderDebugMachine();
  }

  // A local's name shows its memory: a List's buffer as its item type when
  // that is a number type, any other value's own bytes.
  function debugLocalRow(local) {
    const row = debugViewRow([[local.name, "key memory-link"], [local.value, ""], [local.type, "type"]]);
    const item = /^\[(\w+)\]$/.exec(local.type || "");
    const numeric = item && item[1] in DEBUG_MEMORY_TYPES;
    const name = row.firstChild;
    name.title = "Show memory: the bytes of " + local.name;
    name.addEventListener("click", () => debugMemoryOpen(numeric ? local.name + "[0]" : local.name, numeric && item[1] !== "U8" ? item[1] : ""));
    return row;
  }

  // The 0x address the 8 little-endian bytes from `offset` of a read hold.
  function debugMemoryWord(read, offset) {
    const pairs = read.bytes.slice(offset * 2, offset * 2 + 16).match(/../g) || [];
    return "0x" + pairs.reverse().join("");
  }

  function renderDebugMachine() {
    if (typeof viewMode === "undefined" || viewMode !== "debug") return;
    const overlay = debugOverlay;
    const paused = !!(overlay && overlay.runtime_state === "live");
    // Registers: the selected frame's, a value changed since the frame's
    // previous stop marked; a click shows memory where it points.
    const registers = paused && Array.isArray(overlay.registers) ? overlay.registers : [];
    debugViewFill("debug-registers-rows", registers.map((register) => {
      const parts = register.known
        ? [[register.name, "key"], [register.value, "value"], [register.decimal, ""], [register.label, "type"]]
        : [[register.name, "key"], ["not saved", "type"]];
      const row = debugViewRow(parts, register.changed ? "is-changed" : "");
      if (register.known) {
        row.title = "Show memory at $" + register.name;
        row.addEventListener("click", () => debugMemoryOpen("$" + register.name, ""));
      }
      return row;
    }), paused ? "No registers: the selected task has not run" : "Not paused");
    // Disassembly: the rows around the pc, and those asked for before and
    // after them.
    const listed = paused && Array.isArray(overlay.disassembly) ? debugMachine.before.concat(overlay.disassembly, debugMachine.after) : [];
    debugViewFill("debug-disassembly-rows", listed.map((row) => {
      if (!row.valid) return debugViewRow([[row.address, "gutter"], ["(no code here)", "type"]]);
      const where = row.file && row.line ? row.file + ":" + row.line : "";
      return debugViewRow([[row.address, "gutter"], [row.symbol, "key"], [row.text, ""], [where, "type"]], row.here ? "is-active" : "");
    }), paused ? "No code at this stop" : "Not paused");
    renderDebugMemory(paused);
  }

  function renderDebugMemory(paused) {
    const back = debugViewElement("debug-memory-back");
    if (back) back.disabled = !debugMachine.back.length;
    const read = paused ? debugMemoryPage() : null;
    if (!read) {
      const empty = !paused ? "Not paused" : debugMachine.where ? "Reading " + debugMachine.where : "Show memory on a local or register, or type where";
      debugViewFill("debug-memory-rows", [], empty);
      return;
    }
    if (!read.ok) {
      debugViewFill("debug-memory-rows", [debugViewRow([[read.message, ""]])], "");
      return;
    }
    const rows = [debugViewRow([[read.address, "gutter"], [read.heading, "key"]])];
    const pointers = new Set(read.pointers);
    const labels = new Map(read.labels.map((label) => [label.offset, label.text]));
    const size = DEBUG_MEMORY_TYPES[read.type] || 1;
    const count = read.bytes.length / 2;
    const base = BigInt(read.address);
    const skew = Number(base % 8n);
    const marks = { "?": "??", "#": "##", d: "--" };
    for (let start = 0; start < count; start += DEBUG_MEMORY_ROW) {
      const row = debugViewRow([["0x" + (base + BigInt(start)).toString(16).padStart(16, "0"), "gutter"]]);
      const end = Math.min(count, start + DEBUG_MEMORY_ROW);
      // One cell a value (a byte when untyped). Each cell of an aligned word
      // that points into mapped memory is underlined, and a click follows it.
      for (let at = start; at + size <= end; at += size) {
        const cell = document.createElement("span");
        const states = read.state.slice(at, at + size);
        if (read.values.length) cell.textContent = read.values[at / size];
        else cell.textContent = marks[states] || read.bytes.slice(at * 2, at * 2 + 2);
        if (/[?#d]/.test(states)) cell.className = "unmapped";
        const word = size === 1 ? at - (at + skew) % 8 : size === 8 ? at : -1;
        if (word >= 0 && pointers.has(word)) {
          const target = debugMemoryWord(read, word);
          cell.className = "ptr";
          cell.title = "Follow " + target;
          cell.addEventListener("click", () => debugMemoryOpen(target, debugMachine.type));
        }
        row.appendChild(cell);
      }
      if (!read.values.length) {
        let text = "";
        for (let at = start; at < end; at++) {
          const code = parseInt(read.bytes.slice(at * 2, at * 2 + 2), 16);
          text += read.state[at] !== "." ? "." : code >= 32 && code < 127 ? String.fromCharCode(code) : ".";
        }
        const shown = document.createElement("span");
        shown.className = "type";
        shown.textContent = text;
        row.appendChild(shown);
      }
      if (labels.has(start)) {
        const label = document.createElement("span");
        label.className = "key";
        label.textContent = labels.get(start);
        row.appendChild(label);
      }
      rows.push(row);
    }
    if (read.note) rows.push(debugViewRow([[read.note, "type"]]));
    debugViewFill("debug-memory-rows", rows, "");
  }

  // Ask for DEBUG_LISTING_MORE instructions before the first row shown, or
  // after the last.
  function debugListingMore(earlier) {
    if (!debugSessionId || !debugOverlay || !Array.isArray(debugOverlay.disassembly)) return;
    const rows = debugMachine.before.concat(debugOverlay.disassembly, debugMachine.after);
    if (!rows.length) return;
    const edge = earlier ? rows[0] : rows[rows.length - 1];
    debugMachine.listing.push({ kind: "disassembly", from: edge.address, skip: earlier ? -DEBUG_LISTING_MORE : 1, count: DEBUG_LISTING_MORE });
    runDebug([]);
  }

  // A column of panels shares its height among the open ones; a folded
  // panel keeps its header.
  function debugPanelLayout(column) {
    if (!column || (column.id !== "debug-side" && column.id !== "debug-machine")) return;
    column.style.gridTemplateRows = Array.from(column.children).map((panel) => panel.classList.contains("is-folded") ? "auto" : "minmax(0, 1fr)").join(" ");
  }

  function debugPanelFold(panel) {
    const folded = !panel.classList.contains("is-folded");
    panel.classList.toggle("is-folded", folded);
    storeFlag("jet.canvas.debug.folded." + panel.id, folded);
    debugPanelLayout(panel.parentNode);
    if (!folded && debugSessionId && ["debug-registers", "debug-memory", "debug-disassembly"].includes(panel.id)) runDebug([]);
  }

  // Add the machine panels as a third column beside the others, and make
  // each panel's header fold it.
  function bindDebugMachine() {
    const grid = debugViewElement("debug-grid");
    const side = debugViewElement("debug-side");
    if (!grid || !side || debugViewElement("debug-machine")) return;
    const style = document.createElement("style");
    style.textContent = [
      "#debug-grid { grid-template-columns: minmax(0, 3fr) minmax(240px, 2fr) minmax(300px, 2fr); }",
      "#debug-console { grid-column: 1 / -1; }",
      "#debug-machine { display: grid; grid-template-rows: repeat(3, minmax(0, 1fr)); gap: 1px; background: #263b55; min-height: 0; }",
      ".debug-panel h2 { cursor: pointer; }",
      ".debug-panel.is-folded > :not(h2) { display: none !important; }",
      ".debug-panel.is-folded h2::before { content: \"+ \"; }",
      ".debug-panel .row.is-changed .value { color: #fbbf24; }",
      ".debug-panel .row.is-active { background: #1e3a5f; }",
      ".debug-panel .row .gutter { color: #475569; }",
      ".debug-panel .row .ptr { text-decoration: underline; color: #7dd3fc; cursor: pointer; }",
      ".debug-panel .row .unmapped { color: #475569; }",
      ".debug-panel .row .memory-link, #debug-registers-rows .row { cursor: pointer; }",
      ".debug-panel .row .memory-link:hover { text-decoration: underline; }",
      ".debug-panel .tools { display: flex; gap: 6px; padding: 3px 8px; }",
      ".debug-panel .tools input { flex: 1; min-width: 0; }",
      ".debug-panel .tools input, .debug-panel .tools select { background: #07101a; color: #dbeafe; border: 1px solid #263b55; border-radius: 4px; font: 12px ui-monospace, monospace; }"
    ].join("\n");
    document.head.appendChild(style);
    const column = document.createElement("div");
    column.id = "debug-machine";
    const panel = (id, label, tools) => {
      const section = document.createElement("section");
      section.id = id;
      section.className = "debug-panel";
      section.setAttribute("aria-label", label);
      const head = document.createElement("h2");
      head.textContent = label;
      section.appendChild(head);
      if (tools) section.appendChild(tools);
      const rows = document.createElement("div");
      rows.id = id + "-rows";
      section.appendChild(rows);
      column.appendChild(section);
    };
    const tool = (tag, id, text, title) => {
      const element = document.createElement(tag);
      element.id = id;
      if (tag === "input") element.placeholder = text;
      else element.textContent = text;
      element.title = title;
      element.setAttribute("aria-label", title);
      return element;
    };
    const memoryTools = document.createElement("div");
    memoryTools.className = "tools";
    const where = tool("input", "debug-memory-where", "where: a local, 0x address, $register or .section", "Memory to show");
    const type = tool("select", "debug-memory-type", "", "Show memory as");
    for (const name of Object.keys(DEBUG_MEMORY_TYPES)) {
      if (name === "U8") continue;
      const option = document.createElement("option");
      option.value = name;
      option.textContent = name || "bytes";
      type.appendChild(option);
    }
    const back = tool("button", "debug-memory-back", "Back", "Back to the memory shown before");
    memoryTools.append(where, type, back);
    const listingTools = document.createElement("div");
    listingTools.className = "tools";
    listingTools.append(tool("button", "debug-disassembly-earlier", "Earlier", "List the instructions before these"), tool("button", "debug-disassembly-later", "Later", "List the instructions after these"));
    panel("debug-registers", "Registers", null);
    panel("debug-memory", "Memory", memoryTools);
    panel("debug-disassembly", "Disassembly", listingTools);
    grid.insertBefore(column, side.nextSibling);
    where.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter" && where.value.trim()) debugMemoryOpen(where.value.trim(), debugMachine.type);
    });
    type.addEventListener("change", () => {
      debugMachine.type = type.value;
      debugMemoryOpen(debugMachine.where, type.value, false);
    });
    back.addEventListener("click", () => {
      const before = debugMachine.back.pop();
      if (before) debugMemoryOpen(before.where, before.type, false);
    });
    debugViewElement("debug-disassembly-earlier").addEventListener("click", () => debugListingMore(true));
    debugViewElement("debug-disassembly-later").addEventListener("click", () => debugListingMore(false));
    for (const section of document.querySelectorAll("#debug-view .debug-panel")) {
      const head = section.querySelector(":scope > h2");
      if (!head) continue;
      if (debugPanelFolded(section.id)) section.classList.add("is-folded");
      head.title = "Fold or open " + head.textContent;
      head.addEventListener("click", () => debugPanelFold(section));
    }
    debugPanelLayout(side);
    debugPanelLayout(column);
  }

  function debugViewCommand(commands) {
    if (!debugSessionId && commands.length) return;
    runDebug(commands);
  }

  function profileViewUrl() {
    return window.__JET_CANVAS_PROFILE__ || ((window.__JET_CANVAS_BASE__ || "/canvas") + "/profile");
  }

  function profileViewSetState(text) {
    const state = debugViewElement("profile-view-state");
    if (state) state.textContent = text;
  }

  // Ask for the profile of `trace` ("" profiles the entry) over the window
  // `from`..`to` (null for the whole trace).
  function requestProfile(trace, from, to) {
    if (!latestDoc || profileState.busy) return;
    const timeline = debugViewElement("profile-timeline");
    const width = timeline ? Math.max(1, Math.min(4096, Math.floor(timeline.clientWidth || timeline.width) - PROFILE_LABEL_WIDTH)) : 800;
    const body = { schema_version: 1, revision: latestDoc.revision, width };
    const sourceId = currentCanvasSourceId();
    if (sourceId) body.source_id = sourceId;
    if (trace) body.trace = trace;
    if (from !== null && to !== null) {
      body.from_ns = Math.floor(from);
      body.to_ns = Math.ceil(to);
    }
    profileState.busy = true;
    profileViewSetState(trace ? "Reading trace…" : "Profiling the entry…");
    fetch(profileViewUrl(), { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })
      .then((r) => r.json().then((j) => ({ ok: r.ok, json: canvasPayload(j) })))
      .then((result) => {
        profileState.busy = false;
        if (!result.ok || !result.json || result.json.ok === false) {
          const message = ((result.json && result.json.message) || "Profile rejected").split("\n")[0];
          profileViewSetState(message);
          showToast(message, { isError: true });
          return;
        }
        profileState.doc = result.json;
        profileState.trace = result.json.trace || trace;
        profileState.selected = null;
        renderProfileView();
      })
      .catch(() => {
        profileState.busy = false;
        profileViewSetState("Profile request failed");
      });
  }

  function profileViewNs(value) {
    if (value >= 1e9) return (value / 1e9).toFixed(3) + " s";
    if (value >= 1e6) return (value / 1e6).toFixed(3) + " ms";
    if (value >= 1e3) return (value / 1e3).toFixed(1) + " µs";
    return value + " ns";
  }

  function renderProfileView() {
    if (typeof viewMode === "undefined" || viewMode !== "profile") return;
    const doc = profileState.doc;
    const timeline = debugViewElement("profile-timeline");
    if (!timeline) return;
    const width = Math.max(200, timeline.clientWidth || 1200);
    const lanes = (doc && doc.lanes) || [];
    const height = PROFILE_AXIS_HEIGHT + Math.max(4, lanes.length) * PROFILE_LANE_HEIGHT;
    timeline.width = width;
    timeline.height = height;
    const g = timeline.getContext("2d");
    g.fillStyle = "#07101a";
    g.fillRect(0, 0, width, height);
    if (!doc) {
      profileViewSetState("No trace");
      g.fillStyle = "#64748b";
      g.font = "13px system-ui, sans-serif";
      g.fillText("Profile the entry, or open a .jettrace, to draw its timeline.", 16, 32);
      debugViewFill("profile-hot-rows", [], "No samples");
      debugViewFill("profile-detail-rows", [], "Drag across the timeline to zoom; click a mark to see it");
      return;
    }
    const from = doc.window.from_ns;
    const to = Math.max(doc.window.to_ns, from + 1);
    const plot = width - PROFILE_LABEL_WIDTH;
    const x = (ns) => PROFILE_LABEL_WIDTH + (ns - from) / (to - from) * plot;
    profileViewSetState(profileViewNs(to - from) + " shown · " + doc.samples + " samples · " + (profileState.trace || ""));
    // The time axis.
    g.fillStyle = "#8aa4c2";
    g.font = "11px ui-monospace, monospace";
    for (let tick = 0; tick <= 4; tick++) {
      const at = PROFILE_LABEL_WIDTH + plot * tick / 4;
      g.fillRect(at, PROFILE_AXIS_HEIGHT - 6, 1, 6);
      const label = profileViewNs(Math.round((to - from) * tick / 4));
      g.fillText(label, Math.min(at + 2, width - g.measureText(label).width - 2), 12);
    }
    profileState.hits = [];
    lanes.forEach((lane, index) => {
      const top = PROFILE_AXIS_HEIGHT + index * PROFILE_LANE_HEIGHT;
      g.fillStyle = index % 2 ? "#0b1520" : "#0a121c";
      g.fillRect(0, top, width, PROFILE_LANE_HEIGHT);
      g.fillStyle = "#8aa4c2";
      g.fillText(lane.name, 6, top + PROFILE_LANE_HEIGHT / 2 + 4);
      g.fillStyle = PROFILE_LANE_COLORS[lane.name] || "#94a3b8";
      for (const mark of lane.marks) {
        const left = Math.max(PROFILE_LABEL_WIDTH, x(mark.start_ns));
        const right = Math.min(width, x(mark.end_ns));
        const w = Math.max(1, right - left);
        const selected = profileState.selected === mark;
        g.globalAlpha = mark.count > 1 ? 0.7 : 1;
        g.fillRect(left, top + 6, w, PROFILE_LANE_HEIGHT - 12);
        if (selected) {
          g.strokeStyle = "#f8fafc";
          g.strokeRect(left, top + 6, w, PROFILE_LANE_HEIGHT - 12);
        }
        g.globalAlpha = 1;
        if (w > 40) {
          g.save();
          g.fillStyle = "#0a1018";
          g.beginPath();
          g.rect(left, top, w, PROFILE_LANE_HEIGHT);
          g.clip();
          g.fillText(mark.label + (mark.count > 1 ? " ×" + mark.count : ""), left + 3, top + PROFILE_LANE_HEIGHT / 2 + 4);
          g.restore();
          g.fillStyle = PROFILE_LANE_COLORS[lane.name] || "#94a3b8";
        }
        profileState.hits.push({ left, right: left + w, top, bottom: top + PROFILE_LANE_HEIGHT, mark, lane: lane.name });
      }
    });
    if (profileState.dragFrom !== null && profileState.dragTo !== null) {
      g.fillStyle = "rgba(96,165,250,.18)";
      const left = Math.min(profileState.dragFrom, profileState.dragTo);
      g.fillRect(left, 0, Math.abs(profileState.dragTo - profileState.dragFrom), height);
    }
    debugViewFill("profile-hot-rows", (doc.hot_lines || []).map((line) => debugViewRow([[line, ""]])), "No CPU samples in this trace");
    const selected = profileState.selected;
    debugViewFill("profile-detail-rows", selected ? [
      debugViewRow([["label", "key"], [selected.label, ""]]),
      debugViewRow([["start", "key"], [profileViewNs(selected.start_ns), ""]]),
      debugViewRow([["length", "key"], [profileViewNs(selected.end_ns - selected.start_ns), ""]]),
      debugViewRow([["events", "key"], [String(selected.count), ""]])
    ] : [], "Drag across the timeline to zoom; click a mark to see it");
  }

  // The trace time under canvas x `px`.
  function profileViewTime(px) {
    const doc = profileState.doc;
    const timeline = debugViewElement("profile-timeline");
    if (!doc || !timeline) return 0;
    const plot = timeline.width - PROFILE_LABEL_WIDTH;
    const from = doc.window.from_ns;
    const to = Math.max(doc.window.to_ns, from + 1);
    return from + Math.max(0, Math.min(plot, px - PROFILE_LABEL_WIDTH)) / plot * (to - from);
  }

  function profileViewZoom(from, to) {
    const doc = profileState.doc;
    if (!doc || to - from < 1) return;
    profileState.zoom.push([doc.window.from_ns, doc.window.to_ns]);
    requestProfile(profileState.trace, from, to);
  }

  function bindDebugProfileViews() {
    bindDebugMachine();
    const bind = (id, handler) => {
      const element = debugViewElement(id);
      if (element) element.addEventListener("click", handler);
    };
    // The replay and dump controls, beside the run controls: reverse steps
    // and `why` for a replayed session, and opening a `jet run --record`
    // recording or a `.jetdump` as a new session.
    const head = debugViewElement("debug-head");
    if (head && !debugViewElement("debug-view-reverse-step")) {
      const before = debugViewElement("debug-view-state");
      const add = (tag, id, text, title) => {
        const element = document.createElement(tag);
        element.id = id;
        if (tag === "input") {
          element.placeholder = text;
          element.setAttribute("aria-label", title);
        } else {
          element.textContent = text;
          element.title = title;
        }
        head.insertBefore(element, before && before.parentNode === head ? before : null);
      };
      add("button", "debug-view-reverse-step", "Back step", "Step backward, into calls (reverse-step)");
      add("button", "debug-view-reverse-finish", "Back out", "Step backward out to the call (reverse-finish)");
      add("button", "debug-view-reverse-continue", "Back continue", "Run backward to the previous breakpoint (reverse-continue)");
      add("input", "debug-view-why", "why <name>", "Why a value has its value");
      add("input", "debug-view-recording", "recording NAME or .jetproof-replay", "Recording to replay");
      add("button", "debug-view-replay", "Replay", "Debug the entry replaying a jet run --record recording");
      add("input", "debug-view-dump", ".jetdump path", "Dump to open");
      add("button", "debug-view-open-dump", "Open dump", "Debug a .jetdump of the entry");
    }
    const valueOf = (id) => {
      const input = debugViewElement(id);
      return input ? input.value.trim() : "";
    };
    bind("debug-view-run", () => openDebugSession());
    bind("debug-view-continue", () => debugViewCommand(["continue"]));
    bind("debug-view-next", () => debugViewCommand(["next"]));
    bind("debug-view-step", () => debugViewCommand(["step"]));
    bind("debug-view-finish", () => debugViewCommand(["finish"]));
    bind("debug-view-stop", () => stopDebug());
    bind("debug-view-reverse", () => debugViewCommand(["reverse-next"]));
    bind("debug-view-reverse-step", () => debugViewCommand(["reverse-step"]));
    bind("debug-view-reverse-finish", () => debugViewCommand(["reverse-finish"]));
    bind("debug-view-reverse-continue", () => debugViewCommand(["reverse-continue"]));
    bind("debug-view-replay", () => {
      const recording = valueOf("debug-view-recording");
      if (recording) openDebugSession({ tier: "native-recorded", recording });
    });
    bind("debug-view-open-dump", () => {
      const dump = valueOf("debug-view-dump");
      if (dump) openDebugSession({ tier: "native-dump", dump });
    });
    const whyInput = debugViewElement("debug-view-why");
    if (whyInput) whyInput.addEventListener("keydown", (ev) => {
      if (ev.key !== "Enter" || !whyInput.value.trim()) return;
      const name = whyInput.value.trim();
      whyInput.value = "";
      debugViewCommand(["why " + name]);
    });
    const consoleInput = debugViewElement("debug-console-input");
    if (consoleInput) consoleInput.addEventListener("keydown", (ev) => {
      if (ev.key !== "Enter" || !consoleInput.value.trim()) return;
      const command = consoleInput.value.trim();
      consoleInput.value = "";
      if (debugSessionId) runDebug([command]);
      else runDebug([]);
    });
    // A typed watch is anchored to the stop it was typed at, when there is
    // one.
    const watchInput = debugViewElement("debug-watch-input");
    if (watchInput) watchInput.addEventListener("keydown", (ev) => {
      if (ev.key !== "Enter" || !watchInput.value.trim()) return;
      addWatch(watchInput.value.trim(), debugStopIsLive() ? debugOverlay.active_span : null);
      watchInput.value = "";
    });
    document.addEventListener("keydown", (ev) => {
      if (viewMode !== "debug" || (ev.target && ev.target.tagName === "INPUT")) return;
      const commands = { F5: debugSessionId ? ["continue"] : [], F10: ["next"], F11: ev.shiftKey ? ["finish"] : ["step"] };
      if (!(ev.key in commands)) return;
      ev.preventDefault();
      if (ev.key === "F5" && !debugSessionId) runDebug([]);
      else debugViewCommand(commands[ev.key]);
    });
    bind("profile-view-run", () => {
      profileState.zoom = [];
      requestProfile("", null, null);
    });
    bind("profile-view-open", () => {
      const input = debugViewElement("profile-view-trace");
      const path = input ? input.value.trim() : "";
      if (!path) return;
      profileState.zoom = [];
      requestProfile(path, null, null);
    });
    bind("profile-view-zoom-out", () => {
      const back = profileState.zoom.pop();
      if (back) requestProfile(profileState.trace, back[0], back[1]);
    });
    bind("profile-view-reset", () => {
      if (!profileState.trace) return;
      profileState.zoom = [];
      requestProfile(profileState.trace, null, null);
    });
    const timeline = debugViewElement("profile-timeline");
    if (!timeline) return;
    const local = (ev) => {
      const box = timeline.getBoundingClientRect();
      return { x: (ev.clientX - box.left) * (timeline.width / Math.max(1, box.width)), y: (ev.clientY - box.top) * (timeline.height / Math.max(1, box.height)) };
    };
    timeline.addEventListener("mousedown", (ev) => {
      profileState.dragFrom = local(ev).x;
      profileState.dragTo = null;
    });
    timeline.addEventListener("mousemove", (ev) => {
      if (profileState.dragFrom === null) return;
      profileState.dragTo = local(ev).x;
      renderProfileView();
    });
    timeline.addEventListener("mouseup", (ev) => {
      const at = local(ev);
      const from = profileState.dragFrom;
      profileState.dragFrom = null;
      profileState.dragTo = null;
      if (from !== null && Math.abs(at.x - from) > 4) {
        profileViewZoom(profileViewTime(Math.min(from, at.x)), profileViewTime(Math.max(from, at.x)));
        return;
      }
      const hit = (profileState.hits || []).find((h) => at.x >= h.left && at.x <= h.right && at.y >= h.top && at.y <= h.bottom);
      profileState.selected = hit ? hit.mark : null;
      renderProfileView();
    });
    timeline.addEventListener("wheel", (ev) => {
      const doc = profileState.doc;
      if (!doc) return;
      ev.preventDefault();
      const middle = profileViewTime(local(ev).x);
      const span = doc.window.to_ns - doc.window.from_ns;
      const scale = ev.deltaY < 0 ? 0.5 : 2;
      const next = Math.max(1, span * scale);
      const from = Math.max(doc.window.first_ns, middle - (middle - doc.window.from_ns) * scale);
      profileViewZoom(from, Math.min(Math.max(doc.window.last_ns, from + 1), from + next));
    }, { passive: false });
    window.addEventListener("resize", () => renderProfileView());
  }

  bindDebugProfileViews();
