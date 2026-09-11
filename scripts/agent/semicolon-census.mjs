#!/usr/bin/env node
// Census explicit semicolons with the shipped compiler lexer.
//
// This deliberately does not scan Jet text with a JavaScript lexer. The
// compiler's versioned `inspect compiler lex --json` surface already knows
// strings, comments, fences, and synthetic line terminators. An explicit
// semicolon is therefore the compiler token `terminator` whose text is `;`.
//
// Usage: node scripts/agent/semicolon-census.mjs [--json]

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, statSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const JET_ENV = resolve(ROOT, "scripts/agent/jet-env");
const CORPUS_ROOTS = ["examples", "tests", "gauntlet", "docs"];
const SCHEMA = "jet-explicit-semicolon-census-v1";
const GENERATED_BY = "scripts/agent/semicolon-census.mjs";
const COMPILER_COMMAND = "jet inspect compiler lex <file> --json";

function fail(message) {
  throw new Error(`semicolon census: ${message}`);
}

function relativePath(path) {
  return relative(ROOT, path).split("\\").join("/");
}

function walkJetFiles(relativeRoot) {
  const absoluteRoot = resolve(ROOT, relativeRoot);
  if (!existsSync(absoluteRoot)) fail(`missing corpus root ${relativeRoot}`);
  const stat = statSync(absoluteRoot);
  if (stat.isFile()) return absoluteRoot.endsWith(".jet") ? [relativePath(absoluteRoot)] : [];
  if (!stat.isDirectory()) return [];

  return readdirSync(absoluteRoot, { withFileTypes: true })
    .filter((entry) => !entry.isSymbolicLink())
    .filter((entry) => ![".git", "node_modules", "target", "dist", "build"].includes(entry.name))
    .sort((left, right) => left.name.localeCompare(right.name))
    .flatMap((entry) => walkJetFiles(relativePath(resolve(absoluteRoot, entry.name))));
}

function corpusFiles() {
  return [...new Set(CORPUS_ROOTS.flatMap(walkJetFiles))].sort((left, right) => left.localeCompare(right));
}

function compilerLex(file) {
  let output;
  try {
    output = execFileSync(JET_ENV, ["jet", "inspect", "compiler", "lex", file, "--json"], {
      cwd: ROOT,
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
    });
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    fail(`compiler lex failed for ${file}: ${detail}`);
  }

  let envelope;
  try {
    envelope = JSON.parse(output);
  } catch (error) {
    fail(`compiler lex returned non-JSON for ${file}: ${error instanceof Error ? error.message : String(error)}`);
  }
  if (envelope.status !== "ok" || envelope.ok !== true) {
    fail(`compiler lex returned an error for ${file}: ${JSON.stringify(envelope)}`);
  }
  const value = envelope.compiler?.value;
  if (!value || !Array.isArray(value.tokens) || !Array.isArray(value.diagnostics)) {
    fail(`compiler lex response for ${file} lacks the versioned token/diagnostic value`);
  }
  return value;
}

function explicitSemicolon(token) {
  return (
    token?.kind === "terminator"
    && token?.text === ";"
    && token.span?.start < token.span?.end
  );
}

function fileRow(file) {
  const value = compilerLex(file);
  const semicolons = value.tokens.filter(explicitSemicolon).map((token) => ({
    span: token.span,
    start: token.start,
    end: token.end,
  }));
  return {
    path: file,
    explicit_semicolons: semicolons,
    lexical_diagnostics: value.diagnostics.map((diagnostic) => diagnostic.code),
  };
}

export function census() {
  const files = corpusFiles().map(fileRow);
  const filesWithSemicolons = files.filter((row) => row.explicit_semicolons.length > 0);
  return {
    schema: SCHEMA,
    generated_by: GENERATED_BY,
    generated_command: "node scripts/agent/semicolon-census.mjs --json",
    authoritative: {
      synthetic_terminators_are_excluded: "synthetic line terminators have empty text and a zero-length span",
      compiler_command: COMPILER_COMMAND,
      token_kind: "terminator",
      explicit_token_text: ";",
    },
    summary: {
      files_scanned: files.length,
      files_with_explicit_semicolons: filesWithSemicolons.length,
      explicit_semicolons: filesWithSemicolons.reduce(
        (total, row) => total + row.explicit_semicolons.length,
        0,
      ),
      files_with_lexical_diagnostics: files.filter((row) => row.lexical_diagnostics.length > 0).length,
    },
    files: filesWithSemicolons,
  };
}

function usage() {
  return [
    "usage: node scripts/agent/semicolon-census.mjs [--json]",
    "",
    "Walk examples/, tests/, gauntlet/, and docs/ and ask the real compiler lexer",
    "for explicit semicolon tokens. The result is JSON on stdout; it does not",
    "write a generated artifact or modify the corpus.",
  ].join("\n");
}

export function main(argv = process.argv.slice(2)) {
  try {
    if (argv.includes("--help") || argv.includes("-h")) {
      process.stdout.write(`${usage()}\n`);
      return 0;
    }
    const unknown = argv.find((argument) => argument !== "--json");
    if (unknown) fail(`unknown option ${unknown}`);
    process.stdout.write(`${JSON.stringify(census(), null, 2)}\n`);
    return 0;
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    return 1;
  }
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  process.exitCode = main();
}
