
// Canvas capability markers, initial UI setup, endpoint binding, and graph load.
  window.__jetCanvasPinAuthoring = true;
  window.__jetCanvasDebugOverlay = true;
  window.__jetCanvasFrontendFamily = {
    family: "modified_hybrid",
    workbenchProjectViewer: true,
    codeSplitGraphLens: true,
    contextCommands: true,
    dragPinCompatibleMenu: true,
    hoverOnlyTypes: true,
    getterCapsules: true,
    embeddedVariables: true,
    graphiteDetailToggles: ["types", "diagnostics", "effects", "debug", "package"]
  };
  setDeveloperMode(storedFlag("jet.canvas.developerMode"));
  if (coreCatalog) coreCatalog.addEventListener("click", () => openCoreCatalogPalette(""));
  if (toolbarSearch) toolbarSearch.addEventListener("click", () => {
    setDrawer("graphs");
    const searchPanel = document.getElementById("search-panel");
    const detailsEl = searchPanel && searchPanel.querySelector("details");
    if (detailsEl) detailsEl.open = true;
    if (canvasSearch) canvasSearch.focus();
  });
  syncDetailToggles();
  setViewMode("graph");
  details.innerHTML = "<h2>Details</h2><p>Select a node.</p>";
  window.addEventListener("resize", function () {
    if (!compactCanvasMode()) setDrawer(null);
    if (latestDoc) window.requestAnimationFrame(fitGraph);
  });

  const base = window.__JET_CANVAS_BASE__ || "/canvas";
  const graphUrl = window.__JET_CANVAS_GRAPH__ || (base + "/graph");
  const queryUrl = window.__JET_CANVAS_QUERY__ || (base + "/query");
  const coreCatalogUrl = window.__JET_CANVAS_CORE_CATALOG__ || (base + "/core-catalog");
  const sourceControlUrl = window.__JET_CANVAS_SCM__ || (base + "/source-control");
  const proofUrl = window.__JET_CANVAS_PROOF__ || (base + "/proof");
  const commandUrl = window.__JET_CANVAS_COMMAND__ || (base + "/command");
  window.__jetCanvasProofRail = true;
  loadGraph();
