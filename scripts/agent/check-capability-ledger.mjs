#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const MANIFEST_PATH = join(ROOT, "docs/spec/capability-claim-manifest.json");
const REGISTRY_PATH = join(ROOT, "docs/reference/capability-claims.md");
const REPORT_PATH = join(ROOT, "docs/plans/epoch-3/capability-ledger-report.json");
const FIXTURES_PATH = join(ROOT, "tests/fixtures/capability-claims/hostile-cases.json");
const DEFAULT_TOWER_PATH = join(ROOT, ".tower/tower.json");
const EXPECTED_CLASSES = ["reserved", "facade", "partial", "implemented", "proven"];
const PUBLIC_DECLARATION_FILES = ["README.md", "docs/reference/core-library.md", "Source/CLI.rs"];

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function stable(value) {
  if (Array.isArray(value)) return `[${value.map(stable).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stable(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function sameSet(left, right) {
  return left.length === right.length && [...left].sort().every((value, index) => value === [...right].sort()[index]);
}

function registryClaims() {
  const claims = [];
  for (const line of readFileSync(REGISTRY_PATH, "utf8").split(/\r?\n/)) {
    const match = line.match(/^\| `(claim\.[a-z0-9-]+)` \|/);
    if (match) claims.push(match[1]);
  }
  return claims;
}

function declarationClaims() {
  const found = [];
  for (const path of PUBLIC_DECLARATION_FILES) {
    const text = readFileSync(join(ROOT, path), "utf8");
    for (const match of text.matchAll(/CAPABILITY_CLAIM:\s*(claim\.[a-z0-9-]+)/g)) {
      found.push(match[1]);
    }
  }
  return found;
}

function cliCommands() {
  const text = readFileSync(join(ROOT, "Source/CLI.rs"), "utf8");
  return [...text.matchAll(/CommandSpec\s*\{\s*name:\s*"([^"]+)"/g)].map((match) => match[1]);
}

function coreModules() {
  const text = readFileSync(join(ROOT, "crates/jet-foundation/src/Syntax/predicates.rs"), "utf8");
  const block = text.match(/pub const KNOWN_CORE_MODULES:[\s\S]*?= &\[([\s\S]*?)\];/);
  if (!block) throw new Error("KNOWN_CORE_MODULES registry not found");
  return [...block[1].matchAll(/"([^"]+)"/g)].map((match) => match[1]);
}

function towerText(card, field) {
  if (field === "log") return (card.log ?? []).map((entry) => entry.text ?? "").join("\n");
  const value = card[field];
  return typeof value === "string" ? value : stable(value ?? null);
}

function fileText(path, overrides) {
  if (overrides?.has(path)) {
    const value = overrides.get(path);
    if (value === null) throw new Error(`missing proof artifact ${path}`);
    return value;
  }
  const absolute = join(ROOT, path);
  if (!existsSync(absolute)) throw new Error(`missing proof artifact ${path}`);
  return readFileSync(absolute, "utf8");
}

function decisiveContent(text, anchor) {
  const matches = text
    .split(/\r?\n/)
    .filter((line) => line.includes(anchor))
    .map((line) => line.trim().replace(/\s+/g, " "));
  if (matches.length === 0) throw new Error("decisive anchor selected no normalized content");
  return matches.join("\n");
}

function validateArtifact(artifact, ownerCard, overrides) {
  if (!["proof", "supporting", "gap"].includes(artifact.role)) {
    throw new Error(`invalid artifact role ${artifact.role}`);
  }
  if (!artifact.why || artifact.why.length < 20) throw new Error("artifact lacks semantic relevance rationale");
  if (!artifact.anchor || artifact.anchor.length < 8) throw new Error("artifact lacks decisive anchor");
  let text;
  if (artifact.kind === "file") {
    if (!artifact.path || artifact.path.includes("*")) throw new Error("file artifact path must be exact");
    text = fileText(artifact.path, overrides);
  } else if (artifact.kind === "tower") {
    if (!artifact.field) throw new Error("Tower artifact lacks exact field");
    text = towerText(ownerCard, artifact.field);
  } else {
    throw new Error(`unknown artifact kind ${artifact.kind}`);
  }
  if (!text.includes(artifact.anchor)) {
    const where = artifact.kind === "file" ? artifact.path : `Tower #${ownerCard.num}.${artifact.field}`;
    throw new Error(`missing decisive anchor in ${where}: ${artifact.anchor}`);
  }
  const digest = sha256(decisiveContent(text, artifact.anchor));
  if (!artifact.contentDigest) throw new Error("artifact lacks normalized decisive content digest");
  if (digest !== artifact.contentDigest) {
    throw new Error(`decisive content digest mismatch: expected ${artifact.contentDigest}, got ${digest}`);
  }
}

function validateCommand(command, proven, claimId, laneId) {
  if (!Array.isArray(command) || command.length !== 7 || command.some((part) => typeof part !== "string" || !part)) {
    throw new Error("acceptance command must be a non-empty argv array");
  }
  if (command[0] !== "cargo" || command[1] !== "test" || command[2] !== "--test" || command[5] !== "--" || command[6] !== "--exact") {
    throw new Error("acceptance command must name exact cargo test target and `--exact`");
  }
  if (proven) {
    const testPath = join(ROOT, "tests", `${command[3]}.rs`);
    if (!existsSync(testPath)) throw new Error(`proven acceptance test does not exist: ${relative(ROOT, testPath)}`);
    const marker = `CAPABILITY_CLAIM: ${claimId} / ${laneId}`;
    if (!readFileSync(testPath, "utf8").includes(marker)) {
      throw new Error(`acceptance command target lacks lane marker ${marker}`);
    }
  }
}

function validateManifest(manifest, board, options = {}) {
  const errors = [];
  const fail = (claimId, message) => errors.push(`${claimId}: ${message}`);
  if (manifest.schemaVersion !== 2) errors.push(`manifest: unsupported schemaVersion ${manifest.schemaVersion}`);
  if (!sameSet(manifest.classes ?? [], EXPECTED_CLASSES)) errors.push("manifest: evidence classes drifted");

  const claims = new Map();
  for (const claim of manifest.claims ?? []) {
    if (claims.has(claim.id)) fail(claim.id, "duplicate stable claim id");
    claims.set(claim.id, claim);
  }
  const docsClaims = registryClaims();
  if (!sameSet([...claims.keys()], docsClaims)) {
    errors.push(`manifest: docs registry drift; manifest=${[...claims.keys()].sort().join(",")} docs=${docsClaims.sort().join(",")}`);
  }
  const declaredClaims = declarationClaims();
  if (!sameSet([...claims.keys()], [...new Set(declaredClaims)])) {
    errors.push(`manifest: designated public declarations have unowned or missing claim IDs; declarations=${[...new Set(declaredClaims)].sort().join(",")}`);
  }

  for (const [surface, owner] of Object.entries(manifest.cliOwnership ?? {})) {
    if (!claims.has(owner)) errors.push(`cli ${surface}: unknown owner claim ${owner}`);
  }
  for (const [surface, owner] of Object.entries(manifest.coreOwnership ?? {})) {
    if (!claims.has(owner)) errors.push(`core ${surface}: unknown owner claim ${owner}`);
  }
  if (!sameSet(Object.keys(manifest.cliOwnership ?? {}), cliCommands())) {
    errors.push("manifest: Source/CLI.rs has unowned or stale advertised commands");
  }
  if (!sameSet(Object.keys(manifest.coreOwnership ?? {}), coreModules())) {
    errors.push("manifest: KNOWN_CORE_MODULES has unowned or stale advertised modules");
  }

  for (const claim of claims.values()) {
    if (!/^claim\.[a-z0-9-]+$/.test(claim.id)) {
      fail(claim.id, "invalid stable claim id");
      continue;
    }
    if (!EXPECTED_CLASSES.includes(claim.class)) fail(claim.id, `invalid class ${claim.class}`);
    if (!claim.reviewerRationale || claim.reviewerRationale.length < 40) fail(claim.id, "reviewer rationale is not decision-complete");
    const ownerCard = board.cards.find((card) => card.id === claim.owner?.cardId && card.num === claim.owner?.num);
    if (!ownerCard) {
      fail(claim.id, `owner card #${claim.owner?.num} not found`);
      continue;
    }
    if (claim.class === "proven") {
      if (claim.disposition !== "shipped") fail(claim.id, "proven claim must use shipped disposition");
      if (ownerCard.phase !== "done") fail(claim.id, `proven claim owner card is ${ownerCard.phase}, not done`);
    } else {
      if (claim.disposition !== "open") fail(claim.id, "non-proven public claim must remain open; free-text resolution is insufficient");
      if (ownerCard.phase === "done") fail(claim.id, "open claim owner card is done; fake green/log text cannot close it");
    }
    if (!Array.isArray(claim.acceptanceLanes) || claim.acceptanceLanes.length === 0) {
      fail(claim.id, "no exact acceptance lane");
      continue;
    }
    const laneIds = new Set();
    for (const lane of claim.acceptanceLanes) {
      try {
        if (!/^[a-z0-9-]+$/.test(lane.id) || laneIds.has(lane.id)) throw new Error("invalid or duplicate lane id");
        laneIds.add(lane.id);
        if (!lane.why || lane.why.length < 30) throw new Error("lane lacks semantic relevance rationale");
        validateCommand(lane.command, claim.class === "proven", claim.id, lane.id);
        if (!Array.isArray(lane.artifacts) || lane.artifacts.length === 0) throw new Error("lane omitted decisive proof artifacts");
        for (const artifact of lane.artifacts) validateArtifact(artifact, ownerCard, options.fileOverrides);
        if (claim.class === "proven" && lane.artifacts.some((artifact) => artifact.role !== "proof")) {
          throw new Error("proven lane includes non-proof artifact");
        }
        if (claim.class !== "proven" && !lane.artifacts.some((artifact) => artifact.role === "gap")) {
          throw new Error("non-proven lane omitted decisive gap evidence");
        }
      } catch (error) {
        fail(claim.id, `${lane.id ?? "lane"}: ${error.message}`);
      }
    }
  }
  if (errors.length) throw new Error(errors.join("\n"));
}

function reportFor(manifest, board) {
  return {
    schemaVersion: 2,
    inventory: {
      broadClaims: registryClaims().length,
      cliCommands: cliCommands().length,
      coreModules: coreModules().length,
    },
    claims: manifest.claims.map((claim) => {
      const owner = board.cards.find((card) => card.id === claim.owner.cardId);
      return {
        id: claim.id,
        class: claim.class,
        disposition: claim.disposition,
        owner: { cardId: owner.id, num: owner.num, phase: owner.phase },
        lanes: claim.acceptanceLanes.map((lane) => ({
          id: lane.id,
          command: lane.command,
          artifacts: lane.artifacts.map((artifact) => ({
            role: artifact.role,
            locator: artifact.kind === "file" ? artifact.path : `tower:${artifact.field}`,
            anchorDigest: sha256(artifact.anchor),
            contentDigest: artifact.contentDigest,
          })),
        })),
      };
    }),
  };
}

function refreshDigests(towerPath) {
  const manifest = readJson(MANIFEST_PATH);
  const board = readJson(towerPath);
  for (const claim of manifest.claims) {
    const owner = board.cards.find((card) => card.id === claim.owner.cardId && card.num === claim.owner.num);
    if (!owner) throw new Error(`${claim.id}: owner card missing while refreshing digests`);
    for (const lane of claim.acceptanceLanes) {
      for (const artifact of lane.artifacts) {
        try {
          const text = artifact.kind === "file" ? fileText(artifact.path) : towerText(owner, artifact.field);
          artifact.contentDigest = sha256(decisiveContent(text, artifact.anchor));
        } catch (error) {
          throw new Error(`${claim.id}/${lane.id}: ${error.message}`);
        }
      }
    }
  }
  writeFileSync(MANIFEST_PATH, `${JSON.stringify(manifest, null, 2)}\n`);
}

function validateCurrent(towerPath, writeReport = false) {
  const manifest = readJson(MANIFEST_PATH);
  const board = readJson(towerPath);
  validateManifest(manifest, board);
  const report = reportFor(manifest, board);
  if (writeReport) {
    writeFileSync(REPORT_PATH, `${JSON.stringify(report, null, 2)}\n`);
  } else {
    const recorded = readJson(REPORT_PATH);
    if (stable(recorded) !== stable(report)) throw new Error("compact capability report drifted; review manifest/board then regenerate");
  }
  return { manifest, board, report };
}

function verifyFocused(towerPath) {
  const { manifest } = validateCurrent(towerPath);
  for (const claim of manifest.claims.filter((item) => item.class === "proven")) {
    for (const lane of claim.acceptanceLanes) {
      process.stdout.write(`[${claim.id}/${lane.id}] ${lane.command.join(" ")}\n`);
      const result = spawnSync(lane.command[0], lane.command.slice(1), {
        cwd: ROOT,
        env: { ...process.env, TOWER_DATA: towerPath },
        stdio: "inherit",
      });
      if (result.status !== 0) throw new Error(`${claim.id}/${lane.id}: focused acceptance failed (${result.status})`);
    }
  }
}

function hostileFixtures(towerPath) {
  const baseManifest = readJson(MANIFEST_PATH);
  const baseBoard = readJson(towerPath);
  const fixtures = readJson(FIXTURES_PATH);
  for (const fixture of fixtures.cases) {
    const manifest = structuredClone(baseManifest);
    const board = structuredClone(baseBoard);
    const claim = manifest.claims.find((item) => item.id === fixture.claimId);
    const card = board.cards.find((item) => item.num === fixture.cardNum);
    const overrides = new Map();
    if (!claim || !card) throw new Error(`${fixture.id}: real claim/card fixture missing`);
    switch (fixture.mutation) {
      case "fake-green":
        card.phase = "done";
        card.log = [{ at: "fixture", text: "full suite green; done" }, ...(card.log ?? [])];
        break;
      case "unrelated-test":
        claim.acceptanceLanes[0].artifacts[0].path = "tests/cli.rs";
        break;
      case "command-swap":
        claim.acceptanceLanes[0].command = ["cargo", "test", "--test", "truthfulness", "every_feature_example_has_expected_output", "--", "--exact"];
        break;
      case "deferred-as-proven":
        claim.class = "proven";
        claim.disposition = "shipped";
        claim.acceptanceLanes[0].command = ["cargo", "test", "--test", "truthfulness", "every_feature_example_has_expected_output", "--", "--exact"];
        card.phase = "done";
        card.log = [{ at: "fixture", text: "deferred until later; full suite green" }, ...(card.log ?? [])];
        break;
      case "omit-decisive-proof":
        claim.acceptanceLanes[0].artifacts = [];
        break;
      case "false-resolution":
        claim.disposition = "merged";
        card.phase = "done";
        card.log = [{ at: "fixture", text: "merged elsewhere; done" }, ...(card.log ?? [])];
        break;
      case "delete-proof": {
        const artifact = claim.acceptanceLanes[0].artifacts[0];
        overrides.set(artifact.path, null);
        break;
      }
      case "tamper-proof": {
        const artifact = claim.acceptanceLanes[0].artifacts[0];
        overrides.set(artifact.path, fileText(artifact.path).replace(artifact.anchor, "removed claim anchor"));
        break;
      }
      case "corrupt-preserving-anchor": {
        const artifact = claim.acceptanceLanes[0].artifacts[0];
        const text = fileText(artifact.path);
        overrides.set(artifact.path, text.replace(artifact.anchor, `${artifact.anchor} SEMANTICS_CORRUPTED`));
        break;
      }
      default:
        throw new Error(`${fixture.id}: unknown mutation ${fixture.mutation}`);
    }
    let failure = "";
    try { validateManifest(manifest, board, { fileOverrides: overrides }); } catch (error) { failure = error.message; }
    if (!failure.includes(fixture.expectedError)) {
      throw new Error(`${fixture.id}: expected ${fixture.expectedError}, got ${failure || "success"}`);
    }
    process.stdout.write(`${fixture.id}: rejected\n`);
  }
}

function towerPath(args) {
  const index = args.indexOf("--tower");
  return resolve(index >= 0 ? args[index + 1] : process.env.TOWER_DATA ?? DEFAULT_TOWER_PATH);
}

function main() {
  const args = process.argv.slice(2);
  const boardPath = towerPath(args);
  if (args.includes("--generate-report")) {
    validateCurrent(boardPath, true);
    process.stdout.write(`wrote ${relative(ROOT, REPORT_PATH)}\n`);
  } else if (args.includes("--check")) {
    validateCurrent(boardPath);
    process.stdout.write("capability claims: explicit, owned, and current\n");
  } else if (args.includes("--verify-focused")) {
    verifyFocused(boardPath);
  } else if (args.includes("--hostile-fixtures")) {
    hostileFixtures(boardPath);
  } else if (args.includes("--refresh-digests")) {
    refreshDigests(boardPath);
    process.stdout.write(`refreshed normalized decisive-content digests in ${relative(ROOT, MANIFEST_PATH)}\n`);
  } else {
    throw new Error("usage: check-capability-ledger.mjs --refresh-digests|--generate-report|--check|--verify-focused|--hostile-fixtures [--tower PATH]");
  }
}

try { main(); } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 1; }
