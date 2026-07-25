// DOM handles, mutable session state, and rendering constants.
  const canvas = document.getElementById("jet-canvas-view");
  const ctx = canvas.getContext("2d");
  const stage = document.getElementById("stage");
  const sourceView = document.getElementById("source-view");
  const sourceEditor = document.getElementById("source-editor");
  const viewToggle = document.getElementById("view-toggle");
  const lensButtons = Array.from(document.querySelectorAll("[data-view-mode]"));
  const detailToggleInputs = Array.from(document.querySelectorAll("[data-detail-toggle]"));
  const developerModeButton = document.getElementById("developer-mode");
  const toolbarSearch = document.getElementById("toolbar-search");
  const contextMenu = document.getElementById("context-menu");
  const minimap = document.getElementById("minimap");
  const mini = minimap.getContext("2d");
  const details = document.getElementById("details");
  const jump = document.getElementById("jump");
  const graphBack = document.getElementById("graph-back");
  const graphForward = document.getElementById("graph-forward");
  const graphSelect = document.getElementById("graph-select");
  const graphStrip = document.getElementById("graph-strip");
  const graphCount = document.getElementById("graph-count");
  const wireStatus = document.getElementById("wire-status");
  const graphOverview = document.getElementById("graph-overview");
  const leftDrawer = document.getElementById("left-drawer");
  const rightDrawer = document.getElementById("right-drawer");
  const dockGraphs = document.getElementById("dock-graphs");
  const dockDetails = document.getElementById("dock-details");
  const projectRail = document.getElementById("project-rail");
  const projectMode = document.getElementById("project-mode");
  const packageSummary = document.getElementById("package-summary");
  const dependencySummary = document.getElementById("dependency-summary");
  const devSummary = document.getElementById("dev-summary");
  const diagnosticsSummary = document.getElementById("diagnostics-summary");
  const trustSummary = document.getElementById("trust-summary");
  const statusSummary = document.getElementById("status-summary");
  const statusCount = document.getElementById("status-count");
  const problemsPanel = document.getElementById("problems-panel");
  const problemsList = document.getElementById("problems-list");
  const problemsCount = document.getElementById("problems-count");
  const variablesList = document.getElementById("variables-list");
  const variableCount = document.getElementById("variable-count");
  const graphList = document.getElementById("graph-list");
  const canvasSearch = document.getElementById("canvas-search");
  const searchResults = document.getElementById("search-results");
  const zoomLabel = document.getElementById("zoom-label");
  const toolbarZoom = document.getElementById("toolbar-zoom");
  const graphMeta = document.getElementById("graph-meta");
  const sourceId = document.getElementById("source-id");
  const revision = document.getElementById("revision");
  const scmState = document.getElementById("scm-state");
  const proofState = document.getElementById("proof-state");
  const proofRail = document.getElementById("proof-rail");
  const toast = document.getElementById("toast");
  const sourceDiff = document.getElementById("source-diff");
  const editSource = document.getElementById("edit-source");
  const applySourceEdit = document.getElementById("apply-source-edit");
  const cancelSourceEdit = document.getElementById("cancel-source-edit");
  const undoEdit = document.getElementById("undo-edit");
  const redoEdit = document.getElementById("redo-edit");
  const orgAlign = document.getElementById("org-align");
  const orgTidy = document.getElementById("org-tidy");
  const bookmarkAdd = document.getElementById("bookmark-add");
  const bookmarkJump = document.getElementById("bookmark-jump");
  const coreCatalog = document.getElementById("core-catalog");
  const favoriteAction = document.getElementById("favorite-action");
  const checkCurrent = document.getElementById("check-current");
  const runCurrent = document.getElementById("run-current");
  const runHud = document.getElementById("run-hud");
  const firstRunTour = document.getElementById("first-run-tour");
  const tourDismiss = document.getElementById("tour-dismiss");
  const debugStep = document.getElementById("debug-step");
  const debugNext = document.getElementById("debug-next");
  const debugContinue = document.getElementById("debug-continue");
  const debugStop = document.getElementById("debug-stop");
  const debugBreak = document.getElementById("debug-break");
  const debugWatch = document.getElementById("debug-watch");
  let hit = [];
  let pinPoints = new Map();
  let nodeBounds = new Map();
  let pinHit = [];
  let pinEditorHit = [];
  let wireEndpointHit = [];
  let diagnosticHit = [];
  let commentHit = [];
  let connectedPinIds = new Set();
  let latestDoc = null;
  let latestProject = null;
  let selectedSourceId = null;
  let debugOverlay = null;
  let debugState = { breakpoints: [], watches: [] };
  let searchState = { results: [], spans: [], active: -1, diff: null, impact: null };
  let diagnosticsState = { baseRevision: null, diagnosticRevision: null, entries: [], dismissed: new Set() };
  let scm = null;
  let proofDoc = null;
  const UNDO_DEPTH = 50;
  let undoStack = [];
  let redoStack = [];
  let editorState = { bookmarks: [], favorites: [], actionUse: {}, rerouteKnots: [], nodePositions: {}, commentBoxes: [], stagedNodes: [], stagedWires: [], tourDismissed: false };
  let clipboardState = null;
  let wireStyle = "bezier";
  let runState = { running: false, last: "idle" };
  let selectedGraphId = null;
  let selectedVariableName = null;
  let graphBackStack = [];
  let graphForwardStack = [];
  let selectedNodeId = null;
  let selectedNodeIds = new Set();
  let view = { x: 64, y: 42, zoom: 1 };
  let drag = null;
  let lastPointer = { x: 240, y: 140 };
  let nodeOffsets = new Map();
  let autoNodeOffsets = new Map();
  let hoverPin = null;
  let hoverDiagnostic = null;
  let pendingPin = null;
  let spaceDown = false;
  let viewMode = "graph";
  let sourceEditMode = false;
  let detailToggles = { types: false, diagnostics: true, effects: false, debug: false, package: true };
  let developerMode = false;
  const layoutScale = { x: 1.08, y: 1.08 };
  const palette = [
    { title: "Print", detail: "Call print(\"canvas\")", op: "insert_print", node_descriptor_id: "function_exec" },
    { title: "Branch", detail: "Insert if/else rails", op: "insert_branch", node_descriptor_id: "branch" },
    { title: "Switch", detail: "Insert match branches", op: "insert_switch", node_descriptor_id: "dispatch" },
    { title: "Loop", detail: "Insert loop rail", op: "insert_loop", node_descriptor_id: "loop" },
    { title: "Fallible", detail: "Insert ? rail", op: "insert_fallible_rail", node_descriptor_id: "fallible" },
    { title: "Comment", detail: "Add comment box", op: "comment" }
  ];
  let actionEntries = [];
  let coreCatalogLoaded = false;
  let coreCatalogLoading = null;
  let contextMenuState = null;
  let contextMenuOpenedAt = 0;
  let pendingInsertPlacement = null;
  function nodeDescriptorById(id) {
    return id && latestDoc && (latestDoc.node_descriptors || []).find((descriptor) => descriptor.id === id) || null;
  }
  function nodeDescriptor(node) {
    return nodeDescriptorById(node && node.node_descriptor_id);
  }
  function nodeDescriptorForAction(action) {
    if (!action) return null;
    const direct = nodeDescriptorById(action.node_descriptor_id);
    if (direct) return direct;
    const op = action.op || action.insert_op || "";
    const transaction = op === "insert_print" ? "insert_call" : op;
    const archetype = action.pure ? "function_pure" : "function_exec";
    const candidates = latestDoc && (latestDoc.node_descriptors || []).filter((descriptor) => descriptor.transaction === transaction) || [];
    return candidates.find((descriptor) => descriptor.archetype === archetype) || candidates[0] || null;
  }
  function withNodeDescriptor(action) {
    const descriptor = nodeDescriptorForAction(action);
    return descriptor && !action.node_descriptor_id
      ? Object.assign({}, action, { node_descriptor_id: descriptor.id })
      : action;
  }
  window.__jetCanvasTest = window.__jetCanvasTest || {};
  const UI_FONT = '"Inter", "Segoe UI", Roboto, system-ui, sans-serif';
  const MONO_FONT = '"JetBrains Mono", ui-monospace, "SFMono-Regular", Consolas, monospace';
  const TYPE_COLOR_MAP = {
    exec: "#f2f4f8",
    control: "#f2f4f8",
    Bool: "#c0392b",
    Int: "#2ec4b6",
    I64: "#2ec4b6",
    U64: "#2ec4b6",
    Float: "#9acd32",
    F32: "#9acd32",
    F64: "#9acd32",
    String: "#c678dd",
    Char: "#e8a2c8",
    Map: "#e5a03c",
    Result: "#fb7185",
    Struct: "#5b8dd9",
    Enum: "#4f9e5a",
    Fn: "#a78bfa",
    Void: "#6b7280",
    unknown: "#8a8f98"
  };
  const NODE_ARCHETYPE_STYLES = {
    entry: { accent: "#b83647", header: "#b83647", header2: "#6f1e2b", label: "Entry", glyph: "ƒ", subtitle: "" },
    function_exec: { accent: "#4f83b6", header: "#315d84", header2: "#1f3e5b", label: "Function", glyph: "ƒ", subtitle: "" },
    function_pure: { accent: "#4f9e5a", header: "#357745", header2: "#234f31", label: "Pure function", glyph: "ƒ", subtitle: "" },
    control: { accent: "#f2f4f8", header: "#2c333d", header2: "#151a21", label: "Control", glyph: "◇", subtitle: "" },
    value: { accent: "#8a8f98", header: "#252b34", header2: "#151a21", label: "Value", glyph: "•", subtitle: "" }
  };
  const NODE_GRID = 8;
  const NODE_HEADER_H = 26;
  const NODE_ROW_H = 24;
  const NODE_PAD = 8;
  const PIN_DIAMETER = 11;
  const PIN_STROKE = 1.8;
  const COMMENT_TINTS = ["#2563eb", "#2ec4b6", "#4f9e5a", "#e5a03c", "#c678dd", "#8a8f98"];
