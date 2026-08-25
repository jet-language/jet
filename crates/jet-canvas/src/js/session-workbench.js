// Resident session identity, output launcher, preview link, and shared view rail.
  let canvasSession = null;

  function syncCanvasCapabilities(project) {
    const capabilities = project && project.capabilities || {};
    document.querySelectorAll("[data-capability]").forEach((panel) => {
      const capability = panel.getAttribute("data-capability");
      panel.hidden = capabilities[capability] !== true;
    });
    window.__jetCanvasCapabilities = capabilities;
  }

  function canvasClientId() {
    let id = "";
    try { id = sessionStorage.getItem("jet-dev-client") || ""; } catch (_) {}
    if (!id) {
      id = (self.crypto && crypto.randomUUID) ? crypto.randomUUID() : String(Date.now()) + "-canvas";
      try { sessionStorage.setItem("jet-dev-client", id); } catch (_) {}
    }
    return id;
  }

  function canvasSessionPayload(value) {
    if (value && value.session && typeof value.session === "object") return value.session;
    return value && value.schema === "jet.report/v1" && value.canvas && value.canvas.session
      ? value.canvas.session
      : null;
  }

  function syncCanvasSession(session) {
    if (!session || !session.id) return;
    canvasSession = session;
    const shortId = String(session.id).slice(-18);
    const currentRevision = session.source_revision || session.accepted_revision || "uncommitted";
    const identity = document.getElementById("session-identity");
    const footerSession = document.getElementById("session-id");
    if (identity) {
      identity.textContent = `${shortId} · ${session.state || "starting"} · ${session.clients || 0} client${session.clients === 1 ? "" : "s"}`;
      identity.title = session.id;
      identity.dataset.sessionId = session.id;
      identity.dataset.sourceRevision = currentRevision;
    }
    if (footerSession) {
      footerSession.textContent = `session ${shortId}`;
      footerSession.title = session.id;
    }
    document.querySelectorAll("[data-session-view]").forEach((view) => {
      const name = view.getAttribute("data-session-view") || "view";
      view.textContent = `${name} · ${shortId} · ${String(currentRevision).slice(-12)}`;
      view.title = `${name} · ${session.id} · ${currentRevision}`;
      view.dataset.sessionId = session.id;
      view.dataset.sourceRevision = currentRevision;
      view.dataset.sessionState = session.state || "starting";
    });
    const preview = document.getElementById("preview-link");
    const port = session.listeners && session.listeners.application && session.listeners.application.port;
    if (preview && port) {
      preview.href = `${location.protocol}//${location.hostname}:${port}/`;
      preview.textContent = `Open app preview · localhost:${port}`;
    }
    const previewState = document.getElementById("preview-status");
    if (previewState) {
      const good = session.last_good_program || "none";
      previewState.textContent = `${session.state || "starting"} · last good ${good}`;
      previewState.dataset.state = session.state || "starting";
    }
    window.__jetCanvasSession = {
      id: session.id,
      sourceRevision: currentRevision,
      acceptedRevision: session.accepted_revision || null,
      lastGoodRevision: session.last_good_revision || null,
      lastGoodProgram: session.last_good_program || null,
      state: session.state || "starting",
      clients: session.clients || 0,
      history: session.history || { count: 0, receipts: [] },
      listeners: session.listeners || {}
    };
    syncCanvasCapabilities(latestProject);
    syncCanvasOutputs(latestProject);
  }

  function canvasSessionPayloadFromReport(value) {
    const payload = value && value.schema === "jet.report/v1" && value.canvas ? value.canvas : value;
    return payload && payload.session && typeof payload.session === "object" ? payload.session : null;
  }

  function canvasSessionUrl() {
    return (window.__JET_CANVAS_BASE__ || "/canvas") + "/session";
  }

  function loadCanvasSession() {
    return fetch(canvasSessionUrl() + "?client_id=" + encodeURIComponent(canvasClientId()), { cache: "no-store" })
      .then((response) => {
        if (!response.ok) throw new Error("session request failed (" + response.status + ")");
        return response.json();
      })
      .then((value) => {
        const session = canvasSessionPayload(value) || canvasSessionPayloadFromReport(value);
        if (!session) throw new Error("session response has no resident session");
        syncCanvasSession(session);
        return session;
      })
      .catch(() => canvasSession);
  }

  function outputRows(project) {
    if (!project) return [];
    if (Array.isArray(project.outputs) && project.outputs.length) return project.outputs;
    const rows = [];
    for (const pkg of project.packages || []) {
      for (const output of pkg.outputs || []) rows.push(output);
    }
    if (rows.length) return rows;
    return project.targets || [];
  }

  function syncCanvasOutputs(project) {
    const list = document.getElementById("output-list");
    const count = document.getElementById("output-count");
    if (!list) return;
    const rows = outputRows(project);
    list.innerHTML = "";
    if (count) count.textContent = String(rows.length);
    if (!rows.length) {
      const empty = document.createElement("span");
      empty.className = "tag";
      empty.textContent = "No outputs discovered yet";
      list.appendChild(empty);
      return;
    }
    const selected = canvasSession && canvasSession.run && canvasSession.run.output;
    for (const row of rows) {
      const name = String(row.name || row.target || row.output || row.kind || "output");
      const button = document.createElement("button");
      button.type = "button";
      button.className = "project-card output-card" + (name === selected ? " is-active" : "");
      button.dataset.canvasOutput = name;
      button.setAttribute("aria-pressed", name === selected ? "true" : "false");
      const title = document.createElement("b");
      title.textContent = name;
      const detail = document.createElement("small");
      detail.textContent = [row.kind, row.entry || row.path, row.provenance].filter(Boolean).join(" · ") || "valid output";
      button.append(title, detail);
      button.addEventListener("click", () => selectCanvasOutput(name, row.target || name));
      list.appendChild(button);
    }
    window.__jetCanvasOutputs = rows.map((row) => ({
      name: row.name || row.target || row.output || row.kind || "output",
      kind: row.kind || "",
      entry: row.entry || row.path || ""
    }));
  }

  function selectCanvasOutput(output, target) {
    const body = { op: "select_output", output, target, client_id: canvasClientId() };
    return fetch(canvasSessionUrl(), {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body)
    }).then((response) => response.json())
      .then((value) => {
        const session = canvasSessionPayload(value) || canvasSessionPayloadFromReport(value);
        if (session) syncCanvasSession(session);
        showToast("Output selected: " + output);
        return session;
      })
      .catch((error) => showToast(String(error), { isError: true }));
  }

  function canvasSelectedOutput() {
    return canvasSession && canvasSession.run && canvasSession.run.output || "";
  }

  window.__jetCanvasSessionApi = {
    clientId: canvasClientId,
    load: loadCanvasSession,
    selectOutput: selectCanvasOutput,
    selectedOutput: canvasSelectedOutput
  };
