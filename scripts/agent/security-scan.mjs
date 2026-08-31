#!/usr/bin/env node

import {
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const CARD = "#1387";
const INCLUDE_PATHS = Object.freeze(["."]);
const LIMITS = Object.freeze({
  maxFiles: 20_000,
  maxBytes: 1_073_741_824,
});
const CANONICAL_FILES = Object.freeze([
  "scan-manifest.json",
  "findings.json",
  "coverage.json",
]);
const CONTRACT_LIMITS = Object.freeze({
  "scan-manifest.json": 16 * 1024 * 1024,
  "findings.json": 128 * 1024 * 1024,
  "coverage.json": 32 * 1024 * 1024,
});
const EXPLICIT_EXCLUSIONS = Object.freeze([
  {
    pattern: ".git/**",
    reason: "Git control metadata is not committed source.",
  },
  {
    pattern: "target/**",
    reason: "Cargo build output is generated.",
  },
  {
    pattern: "target-*/**",
    reason: "Alternate Cargo build output is generated.",
  },
  {
    pattern: "build/**",
    reason: "Build output is generated.",
  },
  {
    pattern: "result/**",
    reason: "Packaged build output is generated.",
  },
  {
    pattern: ".tmp/**",
    reason: "Agent scratch output is generated outside the source inventory.",
  },
  {
    pattern: ".tmp-*/**",
    reason: "Agent scratch output is generated outside the source inventory.",
  },
  {
    pattern: "node_modules/**",
    reason: "Installed third-party dependencies are not repository source.",
  },
  {
    pattern: ".claude/worktrees/**",
    reason: "Nested agent worktrees are separate working trees.",
  },
  {
    pattern: ".agent-worktrees/**",
    reason: "Nested agent worktrees are separate working trees.",
  },
  {
    pattern: ".agent-scratch-*/**",
    reason: "Agent scratch output is generated.",
  },
  {
    pattern: ".claude/*.log",
    reason: "Agent session logs are generated records, not repository source.",
  },
  {
    pattern: ".claude/*.patch",
    reason: "Agent patch artifacts are generated records, not repository source.",
  },
  {
    pattern: ".claude/bdlog/**",
    reason: "Agent bookkeeping logs are generated records.",
  },
  {
    pattern: ".claude/recovery/**",
    reason: "Agent recovery records are generated records.",
  },
  {
    pattern: ".claude/probe/**",
    reason: "Agent probe output is generated.",
  },
  {
    pattern: "plugins/tower/.tower/**",
    reason: "Tower live state is operational data, not repository source.",
  },
  {
    pattern: "dogfood/tower/tests/parity/fixtures/**",
    reason: "Generated Tower parity fixtures are test output.",
  },
  {
    pattern: "site/dist/**",
    reason: "Site distribution output is generated.",
  },
  {
    pattern: "docs/reference/core-surface-ledger.json",
    reason: "The generated core-surface ledger is derived data.",
  },
  {
    pattern: "docs/audits/security-deep-scan-2026-08-03.md",
    reason: "The canceled discovery report is prior scan evidence, not scan input.",
  },
  {
    pattern: "docs/audits/security-deep-scan-2026-08-03-full.md",
    reason: "The canceled discovery report is prior scan evidence, not scan input.",
  },
  {
    pattern: "docs/audits/security-final-*/**",
    reason: "Prior final reports are outputs and must not become scan input.",
  },
]);
const EXCLUSION_PATTERNS = Object.freeze(
  EXPLICIT_EXCLUSIONS.map((exclusion) => exclusion.pattern),
);
const EXCLUSION_REGEXES = Object.freeze(
  EXCLUSION_PATTERNS.map((pattern) => globToRegex(pattern)),
);

function globToRegex(pattern) {
  let source = "^";
  for (let index = 0; index < pattern.length; index += 1) {
    if (pattern.slice(index, index + 3) === "**/") {
      source += "(?:.*/)?";
      index += 2;
      continue;
    }
    if (pattern.slice(index, index + 3) === "/**") {
      source += "(?:/.*)?";
      index += 2;
      continue;
    }
    if (pattern.slice(index, index + 2) === "**") {
      source += ".*";
      index += 1;
      continue;
    }
    const character = pattern[index];
    if (character === "*") {
      source += "[^/]*";
    } else if (character === "?") {
      source += "[^/]";
    } else if ("\\^$+.|()[]{}".includes(character)) {
      source += "\\" + character;
    } else {
      source += character;
    }
  }
  return new RegExp(source + "$");
}

function fail(message) {
  throw new Error(message);
}

function deepEqual(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function assertEqual(actual, expected, label) {
  if (!deepEqual(actual, expected)) {
    fail(label + " does not match the procedure contract");
  }
}

function assertString(value, label) {
  if (typeof value !== "string" || value.length === 0) {
    fail(label + " must be a non-empty string");
  }
}

function assertArray(value, label) {
  if (!Array.isArray(value)) {
    fail(label + " must be an array");
  }
}

function assertPlainObject(value, label) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    fail(label + " must be an object");
  }
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function readJson(filePath, label) {
  try {
    return JSON.parse(readFileSync(filePath, "utf8"));
  } catch (error) {
    fail("cannot read " + label + ": " + error.message);
  }
}

function ensureDirectory(directory, label) {
  let stats;
  try {
    stats = lstatSync(directory);
  } catch (error) {
    fail(label + " does not exist: " + directory);
  }
  if (stats.isSymbolicLink() || !stats.isDirectory()) {
    fail(label + " must be a real directory: " + directory);
  }
}

function ensureRegular(filePath, label) {
  let stats;
  try {
    stats = lstatSync(filePath);
  } catch (error) {
    fail(label + " does not exist: " + filePath);
  }
  if (stats.isSymbolicLink() || !stats.isFile()) {
    fail(label + " must be a regular file: " + filePath);
  }
  return stats;
}

function resolveRepo(value) {
  const candidate = resolve(value || SCRIPT_REPO_ROOT);
  ensureDirectory(candidate, "repository");
  let repo;
  try {
    repo = realpathSync(candidate);
  } catch (error) {
    fail("cannot resolve repository: " + error.message);
  }
  const reported = runGit(repo, ["rev-parse", "--show-toplevel"]).trim();
  let reportedReal;
  try {
    reportedReal = realpathSync(reported);
  } catch (error) {
    fail("Git did not report a usable repository root: " + error.message);
  }
  if (reportedReal !== repo) {
    fail("repository path is not the Git worktree root");
  }
  return repo;
}

function runGit(repo, argumentsList, options = {}) {
  const result = spawnSync(
    "git",
    ["-C", repo, ...argumentsList],
    options.buffer
      ? { encoding: null }
      : { encoding: "utf8" },
  );
  if (result.error) {
    fail("Git command failed: " + result.error.message);
  }
  if (result.status !== 0) {
    const stderr = result.stderr ? String(result.stderr).trim() : "";
    fail(
      "Git command failed (" +
        argumentsList.join(" ") +
        ")" +
        (stderr ? ": " + stderr : ""),
    );
  }
  return result.stdout;
}

function assertClean(repo) {
  const status = runGit(repo, ["status", "--porcelain=v1", "--untracked-files=all"]);
  if (status.length !== 0) {
    fail("repository must be clean before a security scan");
  }
  const ignored = runGit(
    repo,
    ["status", "--porcelain=v1", "--ignored", "--untracked-files=all", "-z"],
  );
  for (const record of ignored.split("\0").filter(Boolean)) {
    if (!record.startsWith("!! ")) {
      continue;
    }
    const ignoredPath = record.slice(3).replace(/\/$/u, "");
    if (ignoredPath && !isExcluded(ignoredPath)) {
      fail(
        "ignored worktree path is outside the declared exclusions: " +
          ignoredPath,
      );
    }
  }
}

function gitIdentity(repo) {
  const revision = runGit(repo, ["rev-parse", "--verify", "HEAD^{commit}"]).trim();
  const tree = runGit(repo, ["show", "-s", "--format=%T", revision]).trim();
  if (!/^[0-9a-f]{40}$/.test(revision) || !/^[0-9a-f]{40}$/.test(tree)) {
    fail("Git returned an invalid integration revision or tree identity");
  }
  return { revision, tree };
}

function validateGitPath(path) {
  if (
    path.length === 0 ||
    path.startsWith("/") ||
    path.includes("\\") ||
    path.split("/").includes("..") ||
    /[\u0000-\u001f\u007f]/u.test(path)
  ) {
    fail("Git returned an unsafe path: " + JSON.stringify(path));
  }
}

function isExcluded(path) {
  return EXCLUSION_REGEXES.some((regex) => regex.test(path));
}

function buildInventory(repo, revision) {
  const raw = runGit(
    repo,
    ["ls-tree", "-r", "-l", "-z", "--full-tree", revision],
    { buffer: true },
  );
  const entries = raw.toString("utf8").split("\0").filter(Boolean);
  const paths = [];
  let byteCount = 0;
  for (const entry of entries) {
    const separator = entry.indexOf("\t");
    if (separator < 0) {
      fail("Git returned a malformed tree entry");
    }
    const metadata = entry.slice(0, separator).split(/\s+/u);
    const type = metadata[1];
    const size = Number(metadata[3]);
    const path = entry.slice(separator + 1);
    validateGitPath(path);
    if (type !== "blob" || !Number.isSafeInteger(size) || size < 0) {
      fail("the repository contains a non-blob or unbounded tracked entry: " + path);
    }
    if (!isExcluded(path)) {
      paths.push(path);
      byteCount += size;
      if (byteCount > LIMITS.maxBytes) {
        fail(
          "the security scan scope exceeds the byte limit (" +
            byteCount +
            " > " +
            LIMITS.maxBytes +
            ")",
        );
      }
    }
  }
  paths.sort((left, right) =>
    Buffer.from(left, "utf8").compare(Buffer.from(right, "utf8")),
  );
  if (paths.length === 0) {
    fail("the security scan scope is empty after exclusions");
  }
  if (paths.length > LIMITS.maxFiles) {
    fail(
      "the security scan scope exceeds the file limit (" +
        paths.length +
        " > " +
        LIMITS.maxFiles +
        ")",
    );
  }
  if (byteCount > LIMITS.maxBytes) {
    fail(
      "the security scan scope exceeds the byte limit (" +
        byteCount +
        " > " +
        LIMITS.maxBytes +
        ")",
    );
  }
  const text = paths.join("\n") + "\n";
  return {
    paths,
    text,
    fileCount: paths.length,
    byteCount,
    sha256: sha256(text),
  };
}

function resolveNewDirectory(value, label) {
  const directory = resolve(value);
  if (existsSync(directory)) {
    fail(label + " already exists; use a new directory for a fresh run");
  }
  const parent = dirname(directory);
  ensureDirectory(parent, label + " parent");
  return directory;
}

function writeNewFile(filePath, contents, label) {
  try {
    writeFileSync(filePath, contents, { encoding: "utf8", flag: "wx", mode: 0o644 });
  } catch (error) {
    fail("cannot create " + label + ": " + error.message);
  }
}

function prepare(args) {
  const repo = resolveRepo(args.repo);
  assertClean(repo);
  const identity = gitIdentity(repo);
  const inventory = buildInventory(repo, identity.revision);
  const outputDirectory = resolveNewDirectory(args.out, "scan preparation directory");
  mkdirSync(outputDirectory);
  writeNewFile(
    join(outputDirectory, "scope-files.txt"),
    inventory.text,
    "scope inventory",
  );
  const request = {
    documentType: "jet.security-scan.request",
    schemaVersion: "1.0",
    card: CARD,
    scan: {
      mode: "standard",
      coverageMode: "repository",
      fresh: true,
      preparedAt: new Date().toISOString(),
      target: {
        kind: "git_revision",
        revision: identity.revision,
        tree: identity.tree,
        displayName: basename(repo),
      },
      scope: {
        includePaths: INCLUDE_PATHS,
        excludePaths: EXCLUSION_PATTERNS,
        explicitExclusions: EXPLICIT_EXCLUSIONS,
        inventory: {
          path: "scope-files.txt",
          fileCount: inventory.fileCount,
          byteCount: inventory.byteCount,
          sha256: inventory.sha256,
        },
        limits: LIMITS,
      },
      outputs: {
        canonical: CANONICAL_FILES,
        report: "report.md",
        receipt: "security-scan.json",
      },
    },
  };
  writeNewFile(
    join(outputDirectory, "request.json"),
    JSON.stringify(request, null, 2) + "\n",
    "scan request",
  );
  console.log(
    JSON.stringify(
      {
        status: "prepared",
        request: join(outputDirectory, "request.json"),
        inventory: join(outputDirectory, "scope-files.txt"),
        revision: identity.revision,
        tree: identity.tree,
        fileCount: inventory.fileCount,
        byteCount: inventory.byteCount,
        inventorySha256: inventory.sha256,
      },
      null,
      2,
    ),
  );
}

function safeRelativePath(value, label) {
  assertString(value, label);
  if (
    isAbsolute(value) ||
    value.includes("\\") ||
    value.includes("\0") ||
    value.split("/").some((part) => part === ".." || part === ".") ||
    value.split("/").includes("") ||
    value.startsWith("../")
  ) {
    fail(label + " must be a safe relative path: " + JSON.stringify(value));
  }
  return value;
}

function isInside(root, candidate) {
  const child = relative(root, candidate);
  return child === "" || (!child.startsWith(".." + sep) && child !== ".." && !isAbsolute(child));
}

function readRequest(requestPath, repo) {
  const resolvedRequest = resolve(requestPath);
  ensureRegular(resolvedRequest, "scan request");
  const request = readJson(resolvedRequest, "scan request");
  assertPlainObject(request, "scan request");
  if (request.documentType !== "jet.security-scan.request") {
    fail("scan request has the wrong document type");
  }
  if (request.schemaVersion !== "1.0" || request.card !== CARD) {
    fail("scan request has the wrong schema version or card");
  }
  assertPlainObject(request.scan, "scan request scan");
  const scan = request.scan;
  if (scan.mode !== "standard" || scan.coverageMode !== "repository" || scan.fresh !== true) {
    fail("scan request is not a fresh Standard repository scan");
  }
  assertString(scan.preparedAt, "scan.preparedAt");
  if (!Number.isFinite(Date.parse(scan.preparedAt))) {
    fail("scan.preparedAt is not a date-time");
  }
  assertPlainObject(scan.target, "scan.target");
  if (
    scan.target.kind !== "git_revision" ||
    !/^[0-9a-f]{40}$/.test(scan.target.revision) ||
    !/^[0-9a-f]{40}$/.test(scan.target.tree)
  ) {
    fail("scan request target is not a full Git revision identity");
  }
  assertPlainObject(scan.scope, "scan.scope");
  assertEqual(scan.scope.includePaths, INCLUDE_PATHS, "scan includePaths");
  assertEqual(scan.scope.excludePaths, EXCLUSION_PATTERNS, "scan excludePaths");
  assertEqual(scan.scope.explicitExclusions, EXPLICIT_EXCLUSIONS, "scan explicitExclusions");
  assertEqual(scan.scope.limits, LIMITS, "scan limits");
  assertPlainObject(scan.scope.inventory, "scan inventory");
  if (scan.scope.inventory.path !== "scope-files.txt") {
    fail("scan inventory must be scope-files.txt");
  }
  const inventoryPath = join(dirname(resolvedRequest), scan.scope.inventory.path);
  ensureRegular(inventoryPath, "scan inventory");
  const inventoryText = readFileSync(inventoryPath, "utf8");
  const inventory = buildInventory(repo, scan.target.revision);
  if (
    inventoryText !== inventory.text ||
    scan.scope.inventory.fileCount !== inventory.fileCount ||
    scan.scope.inventory.byteCount !== inventory.byteCount ||
    scan.scope.inventory.sha256 !== inventory.sha256
  ) {
    fail("scan inventory does not match the prepared Git revision");
  }
  assertPlainObject(scan.outputs, "scan outputs");
  assertEqual(scan.outputs.canonical, CANONICAL_FILES, "scan canonical outputs");
  if (scan.outputs.report !== "report.md" || scan.outputs.receipt !== "security-scan.json") {
    fail("scan output names do not match the procedure contract");
  }
  assertClean(repo);
  const identity = gitIdentity(repo);
  if (
    identity.revision !== scan.target.revision ||
    identity.tree !== scan.target.tree
  ) {
    fail("repository revision or tree changed after scan preparation");
  }
  return {
    request,
    requestPath: resolvedRequest,
    preparedAt: scan.preparedAt,
    identity,
    inventory,
  };
}

function resolveScanDirectory(value) {
  const directory = resolve(value);
  ensureDirectory(directory, "Codex Security scan directory");
  return realpathSync(directory);
}

function readCanonical(scanDirectory, requireReport = true) {
  const files = {};
  for (const file of CANONICAL_FILES) {
    const filePath = join(scanDirectory, file);
    const stats = ensureRegular(filePath, file);
    const limit = CONTRACT_LIMITS[file];
    if (stats.size > limit) {
      fail(file + " exceeds the Codex Security contract size limit");
    }
    files[file] = readJson(filePath, file);
  }
  const reportPath = join(scanDirectory, "report.md");
  let report = null;
  if (existsSync(reportPath)) {
    ensureRegular(reportPath, "report.md");
    report = readFileSync(reportPath, "utf8");
  } else if (requireReport) {
    fail("report.md does not exist: " + reportPath);
  }
  return {
    manifest: files["scan-manifest.json"],
    findings: files["findings.json"],
    coverage: files["coverage.json"],
    reportPath,
    report,
  };
}

function validateBundleIdentity(bundle, info) {
  const manifest = bundle.manifest;
  const findings = bundle.findings;
  const coverage = bundle.coverage;
  if (
    manifest.documentType !== "codex-security.scan-manifest" ||
    manifest.schemaVersion !== "1.0"
  ) {
    fail("scan manifest does not use the Codex Security sealed schema");
  }
  if (
    findings.documentType !== "codex-security.findings" ||
    findings.schemaVersion !== "1.0" ||
    coverage.documentType !== "codex-security.coverage" ||
    coverage.schemaVersion !== "1.0"
  ) {
    fail("canonical Codex Security documents have the wrong schema");
  }
  assertPlainObject(manifest.scan, "manifest.scan");
  const scan = manifest.scan;
  assertString(scan.id, "manifest.scan.id");
  if (scan.target?.kind !== "git_revision" || scan.target.revision !== info.identity.revision) {
    fail("scan target is not the prepared integration revision");
  }
  if (!deepEqual(scan.scope?.includePaths, INCLUDE_PATHS)) {
    fail("scan manifest includePaths do not match the prepared scope");
  }
  if (!deepEqual(scan.scope?.excludePaths, EXCLUSION_PATTERNS)) {
    fail("scan manifest excludePaths do not match the prepared scope");
  }
  if (findings.scanId !== scan.id || coverage.scanId !== scan.id) {
    fail("canonical documents do not reference the same scan id");
  }
  assertArray(findings.findings, "findings.findings");
  assertArray(coverage.surfaces, "coverage.surfaces");
  return scan;
}

function validateCoverage(coverage) {
  if (
    coverage.mode !== "repository" ||
    coverage.completeness !== "complete" ||
    coverage.inventoryStrategy !== "repository"
  ) {
    fail("coverage is not complete repository coverage");
  }
  assertEqual(coverage.includePaths, INCLUDE_PATHS, "coverage includePaths");
  assertEqual(coverage.excludePaths, EXCLUSION_PATTERNS, "coverage excludePaths");
  assertEqual(
    coverage.explicitExclusions,
    EXPLICIT_EXCLUSIONS,
    "coverage explicitExclusions",
  );
  if (coverage.surfaces.length === 0) {
    fail("coverage has no reviewed surfaces");
  }
  assertArray(coverage.deferred, "coverage.deferred");
  if (coverage.deferred.length !== 0) {
    fail("coverage contains deferred work");
  }
  if (coverage.openQuestions !== undefined) {
    assertArray(coverage.openQuestions, "coverage.openQuestions");
    if (coverage.openQuestions.length !== 0) {
      fail("coverage contains open questions");
    }
  }
  if (containsNeedsFollowUp(coverage.surfaces)) {
    fail("coverage contains a needs_follow_up surface");
  }
}

function containsNeedsFollowUp(value) {
  if (Array.isArray(value)) {
    return value.some((item) => containsNeedsFollowUp(item));
  }
  if (value !== null && typeof value === "object") {
    for (const [key, child] of Object.entries(value)) {
      if (
        (key === "disposition" || key === "status") &&
        child === "needs_follow_up"
      ) {
        return true;
      }
      if (containsNeedsFollowUp(child)) {
        return true;
      }
    }
  }
  return false;
}

function sealedState(scan) {
  const hasSealedAt = Object.hasOwn(scan, "sealedAt");
  const hasArtifacts = Object.hasOwn(scan, "artifacts");
  if (hasSealedAt !== hasArtifacts) {
    fail("scan manifest has only part of the sealed bundle");
  }
  if (hasSealedAt) {
    assertString(scan.sealedAt, "manifest.scan.sealedAt");
    assertArray(scan.artifacts, "manifest.scan.artifacts");
    return true;
  }
  return false;
}

function validateDraft(bundle, info) {
  validateBundleIdentity(bundle, info);
  if (bundle.manifest.scan.status === "failed" || bundle.manifest.scan.status === "canceled") {
    fail("Codex Security scan did not complete");
  }
}

function validateFinalBundle(bundle, info) {
  const scan = validateBundleIdentity(bundle, info);
  if (scan.status !== "completed") {
    fail("Codex Security scan status is not completed");
  }
  if (!Number.isFinite(Date.parse(scan.startedAt))) {
    fail("scan.startedAt is not a date-time");
  }
  if (
    Date.parse(scan.startedAt) < Date.parse(info.preparedAt) ||
    !Number.isFinite(Date.parse(scan.completedAt)) ||
    !Number.isFinite(Date.parse(scan.sealedAt))
  ) {
    fail("scan timestamps do not prove a fresh completed scan");
  }
  if (scan.coverageRef !== "coverage.json" || scan.findingsRef !== "findings.json") {
    fail("scan references are not the canonical bundle files");
  }
  if (
    !Array.isArray(scan.artifacts) ||
    !scan.artifacts.some((artifact) => artifact && artifact.path === "findings.json") ||
    !scan.artifacts.some((artifact) => artifact && artifact.path === "coverage.json")
  ) {
    fail("sealed scan manifest does not list every canonical document");
  }
  validateCoverage(bundle.coverage);
  if (bundle.findings.findings.length !== 0) {
    fail(
      "zero-unresolved gate failed: " +
        bundle.findings.findings.length +
        " reportable finding(s) remain",
    );
  }
  if (
    !bundle.report.includes("### No findings") ||
    !bundle.report.includes("| Reportable findings | 0 |")
  ) {
    fail("report.md is not the generated zero-finding report");
  }
  return {
    scan,
    reportableFindings: bundle.findings.findings.length,
    deferred: bundle.coverage.deferred.length,
    needsFollowUp: 0,
    openQuestions: bundle.coverage.openQuestions
      ? bundle.coverage.openQuestions.length
      : 0,
  };
}

function resolvePluginDirectory(value) {
  const configured = value || process.env.CODEX_SECURITY_PLUGIN_DIR;
  if (!configured) {
    fail("set CODEX_SECURITY_PLUGIN_DIR or pass --plugin-dir");
  }
  const directory = resolve(configured);
  ensureDirectory(directory, "Codex Security plugin directory");
  return realpathSync(directory);
}

function runPluginScript(pluginDirectory, scriptName, argumentsList, repo) {
  const scriptPath = join(pluginDirectory, "scripts", scriptName);
  ensureRegular(scriptPath, scriptName);
  const python =
    process.env.JET_SECURITY_PYTHON ||
    (process.platform === "win32" ? "python" : "python3");
  const result = spawnSync(python, [scriptPath, ...argumentsList], {
    cwd: repo,
    stdio: "inherit",
  });
  if (result.error) {
    fail(scriptName + " failed to start: " + result.error.message);
  }
  if (result.status !== 0) {
    fail(scriptName + " failed with exit code " + String(result.status));
  }
}

function runFinalizer(pluginDirectory, scanDirectory, repo) {
  runPluginScript(
    pluginDirectory,
    "finalize_scan_contract.py",
    ["--scan-dir", scanDirectory, "--source-root", repo],
    repo,
  );
}

function runValidator(pluginDirectory, scanDirectory, repo) {
  runPluginScript(
    pluginDirectory,
    "validate_scan_contract.py",
    ["--scan-dir", scanDirectory],
    repo,
  );
}

function resolvePublishDirectory(repo, value) {
  const directory = resolve(value);
  if (!isInside(repo, directory)) {
    fail("publish directory must be inside the repository");
  }
  const parts = relative(repo, directory).split(sep);
  if (
    parts.length !== 3 ||
    parts[0] !== "docs" ||
    parts[1] !== "audits" ||
    !parts[2].startsWith("security-final-")
  ) {
    fail("publish directory must be docs/audits/security-final-*");
  }
  if (existsSync(directory)) {
    fail("publish directory already exists; refusing to overwrite evidence");
  }
  ensureDirectory(join(repo, "docs"), "docs");
  ensureDirectory(join(repo, "docs", "audits"), "docs/audits");
  return directory;
}

function copyRelativeFile(scanDirectory, publishDirectory, value) {
  const path = safeRelativePath(value, "artifact path");
  const source = resolve(scanDirectory, path);
  const destination = resolve(publishDirectory, path);
  if (!isInside(scanDirectory, source) || !isInside(publishDirectory, destination)) {
    fail("artifact path escapes its bundle: " + path);
  }
  ensureRegular(source, "artifact " + path);
  const sourceReal = realpathSync(source);
  if (!isInside(scanDirectory, sourceReal)) {
    fail("artifact path follows a symlink outside its bundle: " + path);
  }
  ensureRegular(sourceReal, "artifact " + path);
  const parent = dirname(destination);
  mkdirSync(parent, { recursive: true });
  if (existsSync(destination)) {
    fail("duplicate artifact path in published bundle: " + path);
  }
  copyFileSync(source, destination);
}

function linkedEvidencePaths(bundle) {
  const paths = new Set(["scan-manifest.json", "findings.json", "coverage.json", "report.md"]);
  for (const artifact of bundle.manifest.scan.artifacts) {
    assertPlainObject(artifact, "scan artifact");
    paths.add(safeRelativePath(artifact.path, "scan artifact path"));
  }
  for (const finding of bundle.findings.findings) {
    if (finding.writeup?.reportPath) {
      paths.add(safeRelativePath(finding.writeup.reportPath, "finding report path"));
    }
  }
  const portfolioPath = bundle.manifest.scan.hardening?.portfolioPath;
  if (portfolioPath) {
    paths.add(safeRelativePath(portfolioPath, "hardening portfolio path"));
  }
  return [...paths].sort();
}

function artifactDigestMap(directory, paths) {
  const digests = {};
  for (const path of paths) {
    const filePath = join(directory, path);
    ensureRegular(filePath, "published " + path);
    digests[path] = sha256(readFileSync(filePath));
  }
  return digests;
}

function publishBundle(bundle, info, repo, scanDirectory, pluginDirectory, publishValue) {
  const publishDirectory = resolvePublishDirectory(repo, publishValue);
  mkdirSync(publishDirectory);
  for (const path of linkedEvidencePaths(bundle)) {
    copyRelativeFile(scanDirectory, publishDirectory, path);
  }
  writeFileSync(
    join(publishDirectory, "scan-request.json"),
    readFileSync(info.requestPath),
    { flag: "wx", mode: 0o644 },
  );
  writeFileSync(
    join(publishDirectory, "scope-files.txt"),
    info.inventory.text,
    { encoding: "utf8", flag: "wx", mode: 0o644 },
  );
  runValidator(pluginDirectory, publishDirectory, repo);
  const publishedBundle = readCanonical(publishDirectory);
  const gate = validateFinalBundle(publishedBundle, info);
  const digestPaths = [
    ...linkedEvidencePaths(bundle),
    "scan-request.json",
    "scope-files.txt",
  ].sort();
  const result = {
    documentType: "jet.security-scan.result",
    schemaVersion: "1.0",
    card: CARD,
    status: "passed",
    scan: {
      id: gate.scan.id,
      mode: "standard",
      coverageMode: "repository",
      target: {
        kind: "git_revision",
        revision: info.identity.revision,
        tree: info.identity.tree,
      },
      scope: {
        includePaths: INCLUDE_PATHS,
        excludePaths: EXCLUSION_PATTERNS,
        inventory: {
          fileCount: info.inventory.fileCount,
          byteCount: info.inventory.byteCount,
          sha256: info.inventory.sha256,
        },
      },
    },
    gate: {
      reportableFindings: gate.reportableFindings,
      deferred: gate.deferred,
      needsFollowUp: gate.needsFollowUp,
      openQuestions: gate.openQuestions,
      report: "report.md",
    },
    outputs: {
      canonical: CANONICAL_FILES,
      report: "report.md",
      receipt: "security-scan.json",
      request: "scan-request.json",
      inventory: "scope-files.txt",
    },
    sha256: artifactDigestMap(publishDirectory, digestPaths),
  };
  writeNewFile(
    join(publishDirectory, "security-scan.json"),
    JSON.stringify(result, null, 2) + "\n",
    "security scan result",
  );
  return { result, publishDirectory };
}

function finalizeOrCheck(args, command) {
  const repo = resolveRepo(args.repo);
  const info = readRequest(args.request, repo);
  const scanDirectory = resolveScanDirectory(args.scanDir);
  const pluginDirectory = resolvePluginDirectory(args.pluginDir);
  let bundle = readCanonical(scanDirectory, false);
  validateDraft(bundle, info);
  const sealed = sealedState(bundle.manifest.scan);
  if (command === "finalize" && !sealed) {
    runFinalizer(pluginDirectory, scanDirectory, repo);
    bundle = readCanonical(scanDirectory);
  }
  runValidator(pluginDirectory, scanDirectory, repo);
  bundle = readCanonical(scanDirectory);
  const gate = validateFinalBundle(bundle, info);
  let output = {
    status: "passed",
    card: CARD,
    revision: info.identity.revision,
    tree: info.identity.tree,
    scanId: gate.scan.id,
    reportableFindings: gate.reportableFindings,
  };
  if (args.publishDir) {
    const published = publishBundle(
      bundle,
      info,
      repo,
      scanDirectory,
      pluginDirectory,
      args.publishDir,
    );
    output = published.result;
    console.log(
      JSON.stringify(
        {
          ...output,
          publishedDirectory: published.publishDirectory,
        },
        null,
        2,
      ),
    );
    return;
  }
  console.log(JSON.stringify(output, null, 2));
}

function parseArgs(argumentsList) {
  const command = argumentsList[0];
  if (!command || command === "--help" || command === "help") {
    printUsage();
    process.exitCode = command ? 0 : 2;
    return null;
  }
  if (command !== "prepare" && command !== "finalize" && command !== "check") {
    fail("command must be prepare, finalize, or check");
  }
  const args = { command };
  for (let index = 1; index < argumentsList.length; index += 1) {
    const token = argumentsList[index];
    if (args && token === "--repo") {
      args.repo = argumentsList[++index];
    } else if (args && token === "--out") {
      args.out = argumentsList[++index];
    } else if (args && token === "--request") {
      args.request = argumentsList[++index];
    } else if (args && token === "--scan-dir") {
      args.scanDir = argumentsList[++index];
    } else if (args && token === "--plugin-dir") {
      args.pluginDir = argumentsList[++index];
    } else if (args && token === "--publish-dir") {
      args.publishDir = argumentsList[++index];
    } else if (token === "--help") {
      printUsage();
      process.exitCode = 0;
      return null;
    } else {
      fail("unknown argument: " + token);
    }
    if (argumentsList[index] === undefined) {
      fail(token + " requires a value");
    }
  }
  if (command === "prepare" && !args.out) {
    fail("prepare requires --out");
  }
  if ((command === "finalize" || command === "check") && (!args.request || !args.scanDir)) {
    fail(command + " requires --request and --scan-dir");
  }
  if (command === "check" && args.publishDir) {
    fail("check cannot publish evidence");
  }
  return args;
}

function printUsage() {
  console.log(
    [
      "Usage:",
      "  security-scan.mjs prepare --repo REPO --out NEW_DIR",
      "  security-scan.mjs finalize --repo REPO --request REQUEST --scan-dir SCAN_DIR",
      "      --plugin-dir CODEX_SECURITY_PLUGIN_DIR [--publish-dir REPO/docs/audits/security-final-DATE]",
      "  security-scan.mjs check --repo REPO --request REQUEST --scan-dir SCAN_DIR",
      "      --plugin-dir CODEX_SECURITY_PLUGIN_DIR",
    ].join("\n"),
  );
}

try {
  const args = parseArgs(process.argv.slice(2));
  if (args?.command === "prepare") {
    prepare(args);
  } else if (args?.command === "finalize") {
    finalizeOrCheck(args, "finalize");
  } else if (args?.command === "check") {
    finalizeOrCheck(args, "check");
  }
} catch (error) {
  console.error("security-scan: " + error.message);
  process.exitCode = 2;
}
