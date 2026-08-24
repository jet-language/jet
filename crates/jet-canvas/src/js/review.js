// Git text review projection. Diff text stays primary; graph facts are an
// optional mapping over the current source revision.
  let reviewState = {
    sourceControl: null,
    files: [],
    renderedFiles: [],
    selectedPath: null,
    selectedHunkId: null,
    overlayHunkId: null,
    graphLoadingPath: null,
    loading: false,
    error: null
  };

  const reviewFileList = document.getElementById("review-file-list");
  const reviewContent = document.getElementById("review-content");
  const reviewSummary = document.getElementById("review-summary");
  const reviewDevFacts = document.getElementById("review-dev-facts");
  const reviewRefreshButton = document.getElementById("review-refresh");

  function reviewPathKey(path) {
    return String(path || "").replace(/^\.\//, "").replace(/\\/g, "/");
  }

  function reviewPathsMatch(left, right) {
    const a = reviewPathKey(left);
    const b = reviewPathKey(right);
    return !!a && !!b && (a === b || a.endsWith("/" + b) || b.endsWith("/" + a));
  }

  function reviewSourceControlUrl() {
    return window.__JET_CANVAS_SCM__ || ((window.__JET_CANVAS_BASE__ || "/canvas") + "/source-control");
  }

  function reviewLineStarts(source) {
    const starts = [0];
    for (let i = 0; i < source.length; i++) if (source[i] === "\n") starts.push(i + 1);
    return starts;
  }

  function reviewLineSpan(source, line, endLine = line) {
    const starts = reviewLineStarts(source || "");
    const firstLine = Math.max(1, Number(line) || 1);
    const lastLine = Math.max(firstLine, Number(endLine) || firstLine);
    const start = starts[firstLine - 1] === undefined ? source.length : starts[firstLine - 1];
    if (start >= source.length && source.length > 0) return { start: source.length, end: source.length };
    const lastStart = starts[lastLine - 1] === undefined ? source.length : starts[lastLine - 1];
    const newline = source.indexOf("\n", lastStart);
    return { start, end: newline < 0 ? source.length : newline };
  }

  // Canvas graph spans are UTF-8 byte offsets; browser strings use UTF-16
  // offsets. Convert once before comparing a Git line span with a graph node.
  function reviewByteOffsetToTextOffset(source, byteOffset) {
    const target = Math.max(0, Number(byteOffset) || 0);
    let bytes = 0;
    for (let offset = 0; offset < source.length;) {
      if (bytes >= target) return offset;
      const codePoint = source.codePointAt(offset);
      const width = codePoint <= 0x7f ? 1 : codePoint <= 0x7ff ? 2 : codePoint <= 0xffff ? 3 : 4;
      if (bytes + width > target) return offset;
      bytes += width;
      offset += codePoint > 0xffff ? 2 : 1;
    }
    return source.length;
  }

  function reviewTextSpan(source, span) {
    if (!span) return null;
    return {
      start: reviewByteOffsetToTextOffset(source, span.start),
      end: reviewByteOffsetToTextOffset(source, span.end)
    };
  }

  function reviewSpansOverlap(left, right) {
    if (!left || !right) return false;
    if (left.start === left.end) return left.start >= right.start && left.start <= right.end;
    if (right.start === right.end) return right.start >= left.start && right.start <= left.end;
    return left.start <= right.end && right.start <= left.end;
  }

  function reviewParseRange(value) {
    const match = String(value || "").match(/^(\d+)(?:,(\d+))?$/);
    return match ? { start: Number(match[1]), count: Number(match[2] || 1) } : null;
  }

  function reviewParseDiff(diff, path, fileStatus) {
    const lines = String(diff || "").replace(/\r\n/g, "\n").split("\n");
    const hunks = [];
    let hunk = null;
    let hunkNumber = 0;
    let oldLine = 0;
    let newLine = 0;
    const finish = () => {
      if (!hunk) return;
      const additions = hunk.rows.filter((row) => row.prefix === "+").length;
      const deletions = hunk.rows.filter((row) => row.prefix === "-").length;
      hunk.kind = additions && deletions ? "modified" : additions ? "added" : deletions ? "deleted" : "unprojectable";
      hunks.push(hunk);
      hunk = null;
    };
    for (const line of lines) {
      const header = line.match(/^@@ -([^ ]+) \+([^ ]+) @@/);
      if (header) {
        finish();
        const oldRange = reviewParseRange(header[1]);
        const newRange = reviewParseRange(header[2]);
        if (!oldRange || !newRange) continue;
        oldLine = oldRange.start;
        newLine = newRange.start;
        hunk = {
          id: reviewPathKey(path) + "::hunk-" + (++hunkNumber),
          oldStart: oldRange.start,
          oldCount: oldRange.count,
          newStart: newRange.start,
          newCount: newRange.count,
          rows: []
        };
        continue;
      }
      if (!hunk) {
        if (line.startsWith("+") && !line.startsWith("+++")) {
          hunk = {
            id: reviewPathKey(path) + "::hunk-" + (++hunkNumber),
            oldStart: 0,
            oldCount: 0,
            newStart: 1,
            newCount: 0,
            synthetic: true,
            rows: []
          };
          oldLine = 0;
          newLine = 1;
        } else {
          continue;
        }
      }
      if (line.startsWith("\\ No newline")) continue;
      const prefix = line[0];
      if (prefix !== " " && prefix !== "+" && prefix !== "-") continue;
      const row = {
        prefix,
        text: line.slice(1),
        oldLine: prefix === "+" ? null : oldLine++,
        newLine: prefix === "-" ? null : newLine++,
        nodeIds: [],
        span: null,
        status: null
      };
      if (hunk.synthetic && row.newLine !== null) hunk.newCount++;
      hunk.rows.push(row);
    }
    finish();
    if (hunks.length) return hunks;
    if (String(diff || "").includes("Binary files") || String(fileStatus || "").trim().startsWith("R")) {
      return [{
        id: reviewPathKey(path) + "::hunk-1",
        oldStart: 0,
        oldCount: 0,
        newStart: 0,
        newCount: 0,
        kind: "unprojectable",
        binary: true,
        rows: [{ prefix: "!", text: "Git returned no text hunk for this file.", nodeIds: [], span: null, status: "unprojectable" }]
      }];
    }
    return [];
  }

  function reviewStatusClass(status) {
    return "review-status-" + (status || "unprojectable");
  }

  function reviewStatusLabel(status) {
    return status === "unprojectable" ? "text only" : status || "clean";
  }

  function reviewGitFileStatus(status) {
    const code = String(status || "").trim().slice(0, 2);
    if (code === "??" || code.includes("A")) return "added";
    if (code.includes("D")) return "deleted";
    return "modified";
  }

  function reviewCurrentSourcePath() {
    return latestDoc && (latestDoc.source_id || selectedSourceId) || (latestProject && latestProject.entry) || "";
  }

  function reviewCurrentSourceFor(file) {
    if (!latestDoc || !file || !reviewPathsMatch(reviewCurrentSourcePath(), file.path)) return "";
    if (file.revision && latestDoc.revision && file.revision !== latestDoc.revision) return "";
    return latestDoc.source_text || "";
  }

  function reviewCurrentNodesFor(file) {
    if (!latestDoc || !file || !reviewPathsMatch(reviewCurrentSourcePath(), file.path)) return [];
    if (file.revision && latestDoc.revision && file.revision !== latestDoc.revision) return [];
    const source = latestDoc.source_text || "";
    return (latestDoc.graphs || []).flatMap((graph) => graph.nodes || []).map((node) =>
      node.source_span ? Object.assign({}, node, { source_span: reviewTextSpan(source, node.source_span) }) : node
    );
  }

  function reviewCandidates(nodes, span) {
    return nodes.filter((node) => node.source_span && reviewSpansOverlap(node.source_span, span));
  }

  function reviewMapHunk(hunk, source, nodes) {
    const nodeIds = new Set();
    const changedRows = hunk.rows.filter((row) => row.prefix === "+" || row.prefix === "-");
    for (const row of changedRows) {
      if (row.prefix === "+" && row.newLine !== null) {
        row.span = reviewLineSpan(source, row.newLine);
        const matches = reviewCandidates(nodes, row.span);
        row.nodeIds = matches.map((node) => node.node_id);
        row.status = matches.length ? hunk.kind : "unprojectable";
        matches.forEach((node) => nodeIds.add(node.node_id));
      } else {
        row.status = "deleted";
      }
    }
    hunk.nodeIds = Array.from(nodeIds);
    hunk.status = hunk.nodeIds.length ? hunk.kind : hunk.kind === "deleted" ? "deleted" : "unprojectable";
    const currentRows = changedRows.filter((row) => row.newLine !== null);
    hunk.span = currentRows.length
      ? reviewLineSpan(source, Math.min(...currentRows.map((row) => row.newLine)), Math.max(...currentRows.map((row) => row.newLine)))
      : reviewLineSpan(source, hunk.newStart, hunk.newStart);
    return hunk;
  }

  function reviewBuildFile(file) {
    const source = reviewCurrentSourceFor(file);
    const nodes = reviewCurrentNodesFor(file);
    const semanticOps = Array.isArray(file.semantic_ops) ? file.semantic_ops : [];
    const hunks = reviewParseDiff(file.diff, file.path, file.status)
      .map((hunk) => reviewMapHunk(hunk, source, nodes))
      .map((hunk) => Object.assign({}, hunk, { semantic_ops: semanticOps }));
    return Object.assign({}, file, { hunks, semantic_ops: semanticOps, source_text: source });
  }

  function reviewSemanticOpLabel(ops) {
    return (Array.isArray(ops) ? ops : []).map((op) => {
      if (op && op.kind === "rename") return "rename " + (op.from || "?") + " → " + (op.to || "?");
      return op && op.kind ? op.kind : "semantic operation";
    }).join(" · ");
  }

  function reviewNodeTitle(nodeId) {
    if (!latestDoc) return nodeId;
    for (const graph of latestDoc.graphs || []) {
      const node = (graph.nodes || []).find((candidate) => candidate.node_id === nodeId);
      if (node) return node.title || node.kind || nodeId;
    }
    return nodeId;
  }

  function reviewHunkById(id) {
    for (const file of reviewState.renderedFiles || []) {
      const hunk = (file.hunks || []).find((candidate) => candidate.id === id);
      if (hunk) return { file, hunk };
    }
    return null;
  }

  function reviewDeveloperFacts(doc, file) {
    if (!developerMode) return "";
    const facts = [
      doc && doc.protocol,
      doc && doc.project_revision,
      doc && doc.revision,
      latestProject && latestProject.project_revision,
      file && file.revision,
      file && reviewSemanticOpLabel(file.semantic_ops)
    ].filter(Boolean);
    return Array.from(new Set(facts)).join(" · ");
  }

  function reviewTestState() {
    const files = (reviewState.renderedFiles || []).map((file) => ({
      path: file.path,
      status: file.status,
      dirty: !!file.dirty,
      semanticOps: (file.semantic_ops || []).map((op) => ({ kind: op.kind, from: op.from || null, to: op.to || null })),
      hunks: (file.hunks || []).map((hunk) => ({
        id: hunk.id,
        kind: hunk.kind,
        status: hunk.status,
        semanticOps: (hunk.semantic_ops || []).map((op) => ({ kind: op.kind, from: op.from || null, to: op.to || null })),
        nodeIds: hunk.nodeIds || [],
        added: (hunk.rows || []).filter((row) => row.prefix === "+").map((row) => row.text),
        deleted: (hunk.rows || []).filter((row) => row.prefix === "-").map((row) => row.text)
      }))
    }));
    const selected = reviewHunkById(reviewState.selectedHunkId);
    return {
      active: viewMode === "review",
      available: !!(reviewState.sourceControl && reviewState.sourceControl.available),
      dirty: !!(reviewState.sourceControl && reviewState.sourceControl.dirty),
      dirtyFiles: Number(reviewState.sourceControl && reviewState.sourceControl.dirty_files || 0),
      selectedPath: reviewState.selectedPath,
      selectedHunkId: reviewState.selectedHunkId,
      overlayHunkId: reviewState.overlayHunkId,
      selectedNodeIds: selected ? selected.hunk.nodeIds || [] : [],
      files,
      semanticOps: selected ? (selected.file.semantic_ops || []).map((op) => ({ kind: op.kind, from: op.from || null, to: op.to || null })) : [],
      protocol: reviewState.sourceControl && reviewState.sourceControl.protocol || null
    };
  }

  function syncReviewTestState() {
    window.__jetCanvasReview = reviewTestState();
    if (window.__jetCanvasTest) window.__jetCanvasTest.review = window.__jetCanvasReview;
  }

  function syncReviewFromSourceControl(doc) {
    reviewState.sourceControl = doc || null;
    reviewState.error = null;
    reviewState.files = doc && Array.isArray(doc.files) ? doc.files.filter((file) => file.dirty || file.diff || file.status) : [];
    if (!reviewState.files.some((file) => file.path === reviewState.selectedPath)) {
      reviewState.selectedPath = reviewState.files[0] && reviewState.files[0].path || null;
      reviewState.selectedHunkId = null;
      reviewState.overlayHunkId = null;
    }
    if (viewMode === "review") {
      renderReview();
      reviewEnsureGraph(reviewState.selectedPath);
    } else {
      syncReviewTestState();
    }
  }

  function reviewRenderSummary(files) {
    const hunks = files.flatMap((file) => file.hunks || []);
    const mapped = hunks.filter((hunk) => hunk.nodeIds && hunk.nodeIds.length).length;
    const stats = [
      [files.length, "files"],
      [hunks.length, "hunks"],
      [mapped, "mapped"]
    ];
    reviewSummary.innerHTML = stats.map(([value, label]) => `<div class="review-stat"><b>${escapeHtml(value)}</b><span>${escapeHtml(label)}</span></div>`).join("");
  }

  function reviewRenderFiles(files) {
    reviewFileList.innerHTML = files.length ? files.map((file) => {
      const active = file.path === reviewState.selectedPath ? " is-active" : "";
      const status = reviewGitFileStatus(file.status);
      return `<button class="review-file${active}" type="button" data-review-file="${escapeAttr(file.path)}"><i class="review-file-mark"></i><span class="review-file-name">${escapeHtml(file.path)}</span><span class="review-file-status">${escapeHtml(status)}</span><span class="review-file-meta">${file.hunks.length} hunk${file.hunks.length === 1 ? "" : "s"}</span></button>`;
    }).join("") : `<div class="tag">No changed files.</div>`;
  }

  function reviewLineHtml(row) {
    const lineClass = row.prefix === "+" ? "is-add" : row.prefix === "-" ? "is-delete" : "is-context";
    let note = "";
    if (row.prefix === "-") note = "deleted · no current span";
    else if (row.prefix === "+") {
      note = row.nodeIds && row.nodeIds.length
        ? "graph · " + row.nodeIds.slice(0, 2).map(reviewNodeTitle).join(", ")
        : "unprojectable · no current graph span";
    }
    return `<div class="review-line ${lineClass}"><span class="review-line-number">${row.oldLine === null ? "" : escapeHtml(row.oldLine)}</span><span class="review-line-number">${row.newLine === null ? "" : escapeHtml(row.newLine)}</span><span class="review-line-sign">${escapeHtml(row.prefix)}</span><code class="review-line-text">${escapeHtml(row.text)}</code><span class="review-line-note">${escapeHtml(note)}</span></div>`;
  }

  function reviewHunkHtml(file, hunk) {
    const active = hunk.id === reviewState.selectedHunkId ? " is-active" : "";
    const graphDisabled = !(hunk.nodeIds && hunk.nodeIds.length);
    const title = `-${hunk.oldStart},${hunk.oldCount} +${hunk.newStart},${hunk.newCount}`;
    const names = (hunk.nodeIds || []).slice(0, 4).map(reviewNodeTitle).join(", ");
    const mapping = hunk.nodeIds && hunk.nodeIds.length
      ? `<strong>Graph span:</strong> ${escapeHtml(names)}${hunk.nodeIds.length > 4 ? " …" : ""}`
      : hunk.status === "deleted"
        ? "Deleted text has no current graph span."
        : "Text remains visible. Canvas has no current graph span for this hunk.";
    return `<article class="review-hunk${active}" data-review-hunk="${escapeAttr(hunk.id)}" data-review-status="${escapeAttr(hunk.status)}"><header class="review-hunk-head"><div class="review-hunk-title"><span class="review-status ${reviewStatusClass(hunk.status)}">${escapeHtml(reviewStatusLabel(hunk.status))}</span><code>${escapeHtml(title)}</code></div><div class="review-hunk-actions"><button type="button" data-review-source="${escapeAttr(hunk.id)}">Source</button><button type="button" data-review-graph="${escapeAttr(hunk.id)}"${graphDisabled ? " disabled" : ""}>Graph</button></div></header><div class="review-lines">${(hunk.rows || []).map(reviewLineHtml).join("")}</div><div class="review-map">${mapping}</div></article>`;
  }

  function reviewRenderFile(file) {
    if (!file) {
      const singleFile = latestProject && latestProject.mode === "single_file";
      const message = singleFile
        ? "This single-file project has no Git text changes. Edit the Jet source, then refresh Review."
        : "Git text truth reports a clean project. Edit a Jet source file, then refresh Review.";
      reviewContent.innerHTML = `<div class="review-empty" data-review-empty="clean"><h2>No changes to review</h2><p>${escapeHtml(message)}</p></div>`;
      return;
    }
    const status = reviewGitFileStatus(file.status);
    const devFacts = developerMode && file.revision ? `current file ${file.revision}` : "";
    reviewContent.innerHTML = `<div class="review-file-head"><div class="review-file-title"><h2>${escapeHtml(file.path)}</h2><p>${escapeHtml(status)} file · ${file.hunks.length} hunk${file.hunks.length === 1 ? "" : "s"} · Git text truth</p></div><div class="review-file-facts"><span>${escapeHtml(file.status || "dirty")}</span><span class="review-dev">${escapeHtml(devFacts)}</span></div></div><div class="review-legend"><span class="review-status-added"><i></i>added</span><span class="review-status-modified"><i></i>modified</span><span class="review-status-deleted"><i></i>deleted</span><span class="review-status-unprojectable"><i></i>text only</span></div><div class="review-hunks">${file.hunks.length ? file.hunks.map((hunk) => reviewHunkHtml(file, hunk)).join("") : `<div class="review-empty" data-review-empty="unprojectable"><h2>Git returned no text hunks</h2><p>This file is dirty, but Canvas cannot render a text hunk for it. The Git status remains visible.</p></div>`}</div>`;
  }

  function reviewRenderEmpty(title, message, state) {
    reviewState.renderedFiles = [];
    if (reviewDevFacts) reviewDevFacts.textContent = reviewDeveloperFacts(reviewState.sourceControl, null);
    reviewFileList.innerHTML = `<div class="tag">No changed files.</div>`;
    reviewContent.innerHTML = `<div class="review-empty" data-review-empty="${escapeAttr(state || "empty")}"><h2>${escapeHtml(title)}</h2><p>${escapeHtml(message)}</p></div>`;
    reviewSummary.innerHTML = `<div class="review-stat"><b>0</b><span>hunks</span></div>`;
  }

  function renderReview() {
    const doc = reviewState.sourceControl;
    if (reviewState.loading) {
      reviewRenderEmpty("Refreshing Git text truth", "Canvas is reading the current project files and recomputing hunk mappings.", "loading");
      syncReviewTestState();
      return;
    }
    if (reviewState.error) {
      reviewRenderEmpty("Review could not load", reviewState.error, "error");
      syncReviewTestState();
      return;
    }
    if (!doc) {
      reviewRenderEmpty("Review is ready", "Refresh to read Git text truth for this project.", "loading");
      syncReviewTestState();
      return;
    }
    if (!doc.available) {
      const singleFile = latestProject && latestProject.mode === "single_file";
      reviewRenderEmpty(
        singleFile ? "Single-file source is current" : "No Git worktree",
        singleFile
          ? "This single-file project has no Git text baseline, so Review has no diff to show. Canvas still shows the current Jet source."
          : "This folder is not a Git worktree. Canvas still shows the current Jet source.",
        "no-git"
      );
      syncReviewTestState();
      return;
    }
    const files = reviewState.files.map(reviewBuildFile);
    reviewState.renderedFiles = files;
    reviewRenderSummary(files);
    reviewRenderFiles(files);
    const selected = files.find((file) => file.path === reviewState.selectedPath) || files[0] || null;
    if (selected && selected.path !== reviewState.selectedPath) reviewState.selectedPath = selected.path;
    if (selected && !selected.hunks.some((hunk) => hunk.id === reviewState.selectedHunkId)) {
      reviewState.selectedHunkId = selected.hunks[0] && selected.hunks[0].id || null;
    }
    reviewRenderFiles(files);
    reviewRenderFile(selected);
    const selectedHunk = reviewHunkById(reviewState.selectedHunkId);
    if (reviewDevFacts) {
      reviewDevFacts.textContent = reviewDeveloperFacts(doc, selected);
    }
    syncReviewTestState();
    if (selectedHunk && viewMode !== "review" && latestDoc) drawGraph(latestDoc);
  }

  function reviewEnsureGraph(path) {
    if (!path) return Promise.resolve();
    if (reviewState.graphLoadingPath === path) return Promise.resolve();
    const file = (reviewState.renderedFiles || []).find((candidate) => candidate.path === path)
      || (reviewState.files || []).find((candidate) => candidate.path === path);
    if (!file) return Promise.resolve();
    const current = reviewPathsMatch(reviewCurrentSourcePath(), path)
      && latestDoc && (!file.revision || latestDoc.revision === file.revision);
    if (current) return Promise.resolve();
    reviewState.graphLoadingPath = path;
    return loadGraph(path).then(() => {
      reviewState.graphLoadingPath = null;
      if (viewMode === "review") renderReview();
    }).catch(() => {
      reviewState.graphLoadingPath = null;
      if (viewMode === "review") renderReview();
    });
  }

  function reviewSelectFile(path) {
    if (!path) return;
    reviewState.selectedPath = path;
    reviewState.selectedHunkId = null;
    reviewState.overlayHunkId = null;
    renderReview();
    reviewEnsureGraph(path);
  }

  function reviewSelectHunk(id) {
    const found = reviewHunkById(id);
    if (!found) return;
    reviewState.selectedPath = found.file.path;
    reviewState.selectedHunkId = id;
    reviewState.overlayHunkId = id;
    renderReview();
    if (latestDoc) drawGraph(latestDoc);
  }

  function reviewScrollSource(span) {
    if (!span || !sourceView) return;
    const source = latestDoc && latestDoc.source_text || "";
    const line = source.slice(0, span.start).split("\n").length;
    sourceView.scrollTop = Math.max(0, (line - 2) * 12 * 1.6);
  }

  function reviewOpenSource(id) {
    const found = reviewHunkById(id);
    if (!found) return;
    reviewSelectHunk(id);
    reviewEnsureGraph(found.file.path).then(() => {
      if (found.hunk.span) setSourceHash(found.hunk.span);
      setViewMode("code");
      reviewScrollSource(found.hunk.span);
      showToast("Opened source hunk");
    });
  }

  function reviewOpenGraph(id) {
    const found = reviewHunkById(id);
    if (!found) return;
    reviewSelectHunk(id);
    reviewEnsureGraph(found.file.path).then(() => {
      const hunk = reviewHunkById(id);
      if (!hunk || !hunk.hunk.nodeIds.length) {
        setViewMode("graph");
        showToast("Text-only hunk has no current graph span");
        return;
      }
      const nodeId = hunk.hunk.nodeIds[0];
      const graph = (latestDoc && latestDoc.graphs || []).find((candidate) => (candidate.nodes || []).some((node) => node.node_id === nodeId));
      if (graph) switchGraph(graph.graph_id, { nodeId, toast: "Opened review graph span" });
      else {
        setViewMode("graph");
        showToast("Graph span moved");
      }
      syncReviewTestState();
    });
  }

  function reviewRefresh() {
    const path = reviewState.selectedPath;
    reviewState.loading = true;
    reviewState.error = null;
    reviewState.overlayHunkId = null;
    renderReview();
    if (path) reviewState.graphLoadingPath = path;
    const reload = path
      ? Promise.resolve(loadGraph(path)).finally(() => { reviewState.graphLoadingPath = null; })
      : Promise.resolve();
    return Promise.resolve(reload).then(() => fetch(reviewSourceControlUrl(), { cache: "no-store" }))
      .then((response) => response.json().then(canvasPayload))
      .then((doc) => {
        scm = doc;
        reviewState.loading = false;
        reviewState.sourceControl = doc;
        reviewState.files = doc && Array.isArray(doc.files) ? doc.files.filter((file) => file.dirty || file.diff || file.status) : [];
        if (!reviewState.files.some((file) => file.path === reviewState.selectedPath)) reviewState.selectedPath = reviewState.files[0] && reviewState.files[0].path || null;
        renderReview();
        reviewEnsureGraph(reviewState.selectedPath);
        showToast("Review refreshed");
        return doc;
      })
      .catch((error) => {
        reviewState.loading = false;
        reviewState.error = String(error);
        renderReview();
      });
  }

  function loadReview() {
    if (scm) {
      syncReviewFromSourceControl(scm);
      return reviewEnsureGraph(reviewState.selectedPath);
    }
    reviewState.loading = true;
    renderReview();
    return fetch(reviewSourceControlUrl(), { cache: "no-store" })
      .then((response) => response.json().then(canvasPayload))
      .then((doc) => {
        scm = doc;
        reviewState.loading = false;
        syncReviewFromSourceControl(doc);
        return reviewEnsureGraph(reviewState.selectedPath);
      })
      .catch((error) => {
        reviewState.loading = false;
        reviewState.error = String(error);
        renderReview();
      });
  }

  function reviewOverlayForNode(node) {
    const selected = reviewHunkById(reviewState.overlayHunkId);
    if (!selected || !reviewPathsMatch(selected.file.path, reviewCurrentSourcePath()) || !node || !(selected.hunk.nodeIds || []).includes(node.node_id)) return null;
    return selected.hunk.status;
  }

  function reviewStatusColor(status) {
    return status === "added" ? "#5eead4" : status === "modified" ? "#f6d365" : status === "deleted" ? "#fb7185" : "#c084fc";
  }

  function drawReviewGraphOverlay(graph, visibleNodes) {
    const selected = reviewHunkById(reviewState.overlayHunkId);
    if (!selected || !visibleNodes || !visibleNodes.length) return;
    for (const node of visibleNodes) {
      const status = reviewOverlayForNode(node);
      if (!status) continue;
      const size = nodeSize(graph, node);
      const x = sx(nodeX(node));
      const y = sy(nodeY(node));
      const color = reviewStatusColor(status);
      ctx.save();
      ctx.setLineDash([9, 5]);
      ctx.lineWidth = Math.max(2, 2.5 * view.zoom);
      ctx.strokeStyle = color;
      ctx.shadowColor = color;
      ctx.shadowBlur = 12;
      roundRect(x - 5 * view.zoom, y - 5 * view.zoom, size.w * view.zoom + 10 * view.zoom, size.h * view.zoom + 10 * view.zoom, 10 * view.zoom);
      ctx.stroke();
      ctx.restore();
    }
    const size = cssSize();
    const color = reviewStatusColor(selected.hunk.status);
    const text = selected.hunk.nodeIds.length
      ? reviewStatusLabel(selected.hunk.status) + " hunk · graph span highlighted"
      : reviewStatusLabel(selected.hunk.status) + " hunk · text only";
    const width = Math.min(430, Math.max(230, size.width - 24));
    roundRect(12, 54, width, 32, 6);
    ctx.fillStyle = "rgba(8,17,29,.92)";
    ctx.fill();
    ctx.strokeStyle = color;
    ctx.lineWidth = 1;
    ctx.stroke();
    ctx.fillStyle = color;
    ctx.font = "11px ui-monospace, Consolas, monospace";
    ctx.fillText(text, 23, 74);
  }

  if (reviewFileList) reviewFileList.addEventListener("click", (event) => {
    const button = event.target.closest("[data-review-file]");
    if (button) reviewSelectFile(button.getAttribute("data-review-file"));
  });
  if (reviewContent) reviewContent.addEventListener("click", (event) => {
    const source = event.target.closest("[data-review-source]");
    const graph = event.target.closest("[data-review-graph]");
    const hunk = event.target.closest("[data-review-hunk]");
    if (source) return reviewOpenSource(source.getAttribute("data-review-source"));
    if (graph) return reviewOpenGraph(graph.getAttribute("data-review-graph"));
    if (hunk) reviewSelectHunk(hunk.getAttribute("data-review-hunk"));
  });
  if (reviewRefreshButton) reviewRefreshButton.addEventListener("click", reviewRefresh);
  if (developerModeButton) developerModeButton.addEventListener("click", () => {
    if (viewMode === "review") renderReview();
  });
