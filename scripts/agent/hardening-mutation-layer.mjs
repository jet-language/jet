#!/usr/bin/env node

import { spawn, spawnSync } from "node:child_process";
import {
  closeSync,
  existsSync,
  fsyncSync,
  mkdirSync,
  openSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  makeResultBundle,
  canonicalJson,
  serializeBundles,
  sha256,
  tierCommand,
} from "./hardening-oracle-layer.mjs";

/**
 * Layer 4 (#2343). Mutation descriptors are semantic AST operations. The
 * adapter below uses a dependency-free Rust token AST so it can run in the
 * compiler repository without adding a parser dependency.
 */

export const MUTATION_SCHEMA = "jet.hardening.mutation.v1";
export const MUTATION_SCHEMA_VERSION = 1;
export const MUTATION_MAX_CASES = 16;
export const MUTATION_DEFAULT_SEED = "2343";

const WITNESS_SOURCE = "fn run() {\n    value :: 1\n    print(value)\n}\n";
const REQUIRED_FIELDS = Object.freeze(["id", "seam", "expected_layer", "ast_mutation", "witness"]);
export const EXPECTED_KILLER_LAYERS = Object.freeze(["conformance", "oracle", "differential", "property", "grammar"]);
export const CRITICAL_SILENT_DATA_SEAMS = Object.freeze([
  "route/coverage omission",
  "semantic equality recursion",
  "indexed-place lowering",
  "packed-Int tag/arena identity",
  "typed empty-map generics",
  "release emission totality",
  "input transport",
  "optimizer branch selection",
  "observable-sink removal",
]);

function clone(value) {
  if (value === undefined) return undefined;
  return JSON.parse(JSON.stringify(value));
}

function freezeDeep(value) {
  if (!value || typeof value !== "object" || Object.isFrozen(value)) return value;
  for (const child of Object.values(value)) freezeDeep(child);
  return Object.freeze(value);
}

function stable(value) {
  return canonicalJson(value);
}

function descriptor({
  id,
  seam,
  expected_layer,
  source_file,
  symbol,
  selector,
  path,
  operation,
  replacement = null,
  witness_source = WITNESS_SOURCE,
  witness_sink = "print(value)",
  witness_normalization = [],
  proof_relation,
}) {
  return {
    id,
    seam,
    expected_layer,
    ast_mutation: {
      language: "rust",
      source_file,
      symbol,
      node_kind: selector.node_kind,
      selector: clone(selector),
      path,
      operation,
      replacement: clone(replacement),
    },
    witness: {
      source: witness_source,
      value_consuming: true,
      sink: witness_sink,
      normalization: [...witness_normalization],
    },
    proof_relation,
  };
}

/**
 * Reviewed critical seam catalog. `selector` and `path` are consumed by an
 * AST adapter, not by a source-text replace. Paths are stable semantic node
 * paths; source_file and symbol make each target auditable against the Rust
 * source seam. A missing node is a mutation application error.
 */
export const MUTATION_CATALOG = freezeDeep([
  descriptor({
    id: "mutant.semantic-equality-recursion",
    seam: "semantic equality recursion",
    expected_layer: "property",
    source_file: "crates/jet-comptime/src/Comptime/Builtins.rs",
    symbol: "values_equal",
    selector: {
      node_kind: "match_arm",
      field: "expression",
      value: "list_values_equal(left, right)",
      pattern: "(CtValue::List(left), CtValue::List(right))",
    },
    path: ["functions", "values_equal", "list_arm", "expression"],
    operation: "replace",
    replacement: { kind: "expression", source: "left.len() == right.len()" },
    witness_source: `@left :: [[1, 2], [3]]
@right :: [[1, 9], [3]]
@equal :: @left == @right

fn run() {
    print(@equal)
}
`,
    witness_sink: "print(@equal)",
    proof_relation: "nested values_equal must recurse into collection elements and print false",
  }),
  descriptor({
    id: "mutant.semantic-equality-ct-value-partial-eq",
    seam: "semantic equality recursion",
    expected_layer: "property",
    source_file: "crates/jet-foundation/src/AST/comptime.rs",
    symbol: "impl PartialEq for CtValue::eq",
    selector: {
      node_kind: "match_arm",
      field: "expression",
      value: "left == right",
      pattern: "(Self::List(left), Self::List(right))",
    },
    path: ["impls", "CtValue", "PartialEq", "list_arm", "expression"],
    operation: "replace",
    replacement: { kind: "expression", source: "left.len() == right.len()" },
    witness_source: `fn run() {
    left :: [[1, 2], [3]]
    right :: [[1, 9], [3]]
    print(left == right)
}
`,
    witness_sink: "print(left == right)",
    proof_relation: "CtValue PartialEq must compare nested list elements, not only list lengths",
  }),
  descriptor({
    id: "mutant.indexed-place-lowering",
    seam: "indexed-place lowering",
    expected_layer: "property",
    source_file: "crates/jet-codegen/src/Codegen/TIR/lower/expressions.rs",
    symbol: "lower_expr_as_mut_place",
    selector: {
      node_kind: "match_arm",
      field: "base",
      value: "lower_expr_as_mut_place(base, cx, env)",
      pattern: "Expr::Index { base, index, kind }",
    },
    path: ["functions", "lower_expr_as_mut_place", "index_arm", "base"],
    operation: "replace",
    replacement: { kind: "expression", source: "lower_expr(base, cx, env)" },
    witness_source: `fn run() {
    outer := [[1, 2], [3]]
    outer[0].push(9)
    print(outer)
}
`,
    witness_sink: "print(outer)",
    proof_relation: "nested-place mutation must update the original collection",
  }),
  descriptor({
    id: "mutant.indexed-place-emission",
    seam: "indexed-place lowering",
    expected_layer: "property",
    source_file: "crates/jet-codegen/src/Codegen/TIR/emit/statements.rs",
    symbol: "emit_mut_collection_place",
    selector: {
      node_kind: "call_expression",
      field: "base_place",
      value: "emit_mut_collection_place(base, cx, cleanups)",
      pattern: "TExprKind::Index { base, index, is_map, line, .. }",
    },
    path: ["functions", "emit_mut_collection_place", "index_arm", "base_place"],
    operation: "replace",
    replacement: { kind: "expression", source: "emit_expr_with_cleanups(base, cx, cleanups)" },
    witness_source: `fn run() {
    outer := [[1, 2], [3]]
    outer[0].push(9)
    print(outer)
}
`,
    witness_sink: "print(outer)",
    proof_relation: "the emitter must retain a live indexed place instead of emitting a cloned value",
  }),
  descriptor({
    id: "mutant.packed-int-tag-arena",
    seam: "packed-Int tag/arena identity",
    expected_layer: "property",
    source_file: "crates/jet-codegen/src/Prelude/CoreLib/JetStd/CommonTypes.rs",
    symbol: "jet_int_pack",
    selector: {
      node_kind: "if_expression",
      field: "deduplication",
      value: "values.iter().position(|existing| existing == &value)",
      pattern: "fn jet_int_pack(value: JetBigInt) -> i64",
    },
    path: ["modules", "JetStd", "CommonTypes", "jet_int_pack", "deduplication"],
    operation: "replace",
    replacement: { kind: "expression", source: "values.iter().position(|existing| existing != &value)" },
    witness_source: `fn run() {
    values := [Int:String]{}
    min_inserted := values.add(Int.MIN, "min")
    max_inserted := values.add(Int.MAX, "max")
    print(min_inserted, max_inserted)
    print(values.get(Int.MIN) ?? "missing")
    print(values.get(Int.MAX) ?? "missing")
}
`,
    witness_sink: "print(values.get(Int.MAX) ?? \"missing\")",
    proof_relation: "packed extreme Int keys must resolve through the same arena identity on insert and lookup",
  }),
  descriptor({
    id: "mutant.release-emission-totality",
    seam: "release emission totality",
    expected_layer: "grammar",
    source_file: "crates/jet-codegen/src/Codegen/TIR/emit/statements.rs",
    symbol: "emit_tir_stmt::TStmt::DeferClose",
    selector: {
      node_kind: "string_literal",
      field: "format_template",
      value: "\"{}let mut {jet_prefix}deferred_close_{} = JetDeferredClose::new(move || {{ let _ = {}; }});\\n\"",
      pattern: "TStmt::DeferClose { close, resource, id }",
    },
    path: ["functions", "emit_tir_stmt", "defer_close_arm", "format_template"],
    operation: "replace",
    replacement: { kind: "string_literal", source: "\"{}let mut {jet_prefix}deferred_close_{} = JetDeferredClose::new(move || {{ let _ = (); }});\\n\"" },
    witness_source: `struct Resource {
    name: String
}

impl Resource.Close {
    fn close(^self) { print("close {self.name}") }
}

fn make() Int -> {
    first := Resource{name: "first"}
    defer close(^first)
    print(first.name)
    print("body")
    return 7
}

fn run() {
    print(make())
}
`,
    witness_sink: "print(make())",
    proof_relation: "accepted defer syntax must remain total and preserve release cleanup output",
  }),
  descriptor({
    id: "mutant.release-task-all-emission",
    seam: "release emission totality",
    expected_layer: "grammar",
    source_file: "crates/jet-codegen/src/Codegen/TIR/emit/expressions.rs",
    symbol: "emit_tir_expr::TExprKind::TaskGroupAll",
    selector: {
      node_kind: "match_arm",
      field: "helper",
      value: "\"jet_task_all_result\"",
      pattern: "TExprKind::TaskGroupAll { tasks }",
    },
    path: ["functions", "emit_tir_expr", "task_group_all_arm", "result_helper"],
    operation: "replace",
    replacement: { kind: "string_literal", source: "\"jet_task_all\"" },
    witness_source: `fn make(first: Int, last: Int) [Int:Int] -> {
    out := [Int:Int]{}
    loop n in first..last -> out[n] = n * 3
    print(out.len())
    return out
}

fn run() {
    maps :: task.all {
        make(1, 25),
        make(26, 50)
    } ?? panic("task.all failed")
    print(maps[0].len(), maps[1].get(26) ?? -1)
}
`,
    witness_sink: "print(maps[0].len(), maps[1].get(26) ?? -1)",
    proof_relation: "release task.all must select the result-carrier helper and consume every branch value",
  }),
  descriptor({
    id: "mutant.typed-empty-map-generics",
    seam: "typed empty-map generics",
    expected_layer: "grammar",
    source_file: "crates/jet-codegen/src/Codegen/TIR/emit/expressions.rs",
    symbol: "emit_tir_expr::TExprKind::MapLit",
    selector: {
      node_kind: "string_literal",
      field: "format_template",
      value: "\"{root}JetMap::new()\"",
      pattern: "TExprKind::MapLit(entries) if entries.is_empty()",
    },
    path: ["functions", "emit_tir_expr", "map_lit_arm", "empty_constructor", "format_template"],
    operation: "replace",
    replacement: { kind: "string_literal", source: "\"{root}JetMap::<Int, String>::new()\"" },
    witness_source: `fn run() {
    scores := [String:Int]{}
    scores["answer"] = 42
    print(scores["answer"])
}
`,
    witness_sink: "print(scores[\"answer\"])",
    proof_relation: "typed empty map emission must preserve declared key/value generic order",
  }),
  descriptor({
    id: "mutant.route-coverage-omission",
    seam: "route/coverage omission",
    expected_layer: "conformance",
    source_file: "crates/jet-foundation/src/Syntax/core_calls.rs",
    symbol: "CORE_CALL_AMBIENT_ROUTES",
    selector: {
      node_kind: "array_element",
      field: "route",
      value: '("core.term", "read_all_input")',
      pattern: "CORE_CALL_AMBIENT_ROUTES: &[(&str, &str)]",
    },
    path: ["constants", "CORE_CALL_AMBIENT_ROUTES", "entries", "core.term.read_all_input"],
    operation: "delete",
    witness_source: `use core.term as io

fn run() {
    value :: io.read_all_input() ?? ""
    print(value)
}
`,
    witness_sink: "print(value)",
    proof_relation: "a registered ambient route must reach the same value-consuming read_all_input conformance witness",
  }),
  descriptor({
    id: "mutant.input-transport",
    seam: "input transport",
    expected_layer: "differential",
    source_file: "crates/jet-codegen/src/Prelude/CoreLib/Top/Process.rs",
    symbol: "jet_process_command_base_with_identity",
    selector: {
      node_kind: "match_arm",
      field: "stdin_mapping",
      value: "command.stdin(match &spec.stdin { Some(mode) => jet_process_stdio(mode), None => std::process::Stdio::null() })",
      pattern: "command.stdin(match &spec.stdin {\n        Some(mode) => jet_process_stdio(mode),\n        None => std::process::Stdio::null(),\n    })",
    },
    path: ["functions", "jet_process_command_base_with_identity", "stdin", "mapping"],
    operation: "replace",
    replacement: { kind: "expression", source: "std::process::Stdio::null()" },
    witness_source: `use core.process as process

fn run() {
    child :: process.cmd(["sh", "-c", "read line; printf '%s' \\\"$line\\\""])
        .stdin(.Capture)
        .stdout(.Capture)
        .spawn() ?? panic("spawn failed")
    write_ok :: child.stdin.write("transport-ok\\n") ?? panic("write failed")
    print(write_ok)
    result :: child.wait() ?? panic("wait failed")
    print(result.output)
}
`,
    witness_sink: "print(result.output)",
    witness_normalization: ["process.stdin.mode"],
    proof_relation: "captured stdin must cross the process boundary and print transport-ok",
  }),
  descriptor({
    id: "mutant.optimizer-branch-selection",
    seam: "optimizer branch selection",
    expected_layer: "differential",
    source_file: "crates/jet-codegen/src/Codegen/TIR/emit/statements.rs",
    symbol: "emit_tir_stmt::Branch::SparseInt",
    selector: {
      node_kind: "unary_expression",
      field: "body",
      value: "*value",
      pattern: "sparse_arms.sort_unstable_by_key(|(value, _)| *value)",
    },
    path: ["functions", "emit_tir_stmt", "sparse_branch", "sort_key", "body"],
    operation: "replace",
    replacement: { kind: "expression", source: "-*value" },
    witness_source: `fn run() {
    value :: 1000
    result :: if value == {
        1 -> "one"
        1000 -> "thousand"
        1000000 -> "million"
        else -> "other"
    }
    print(result)
}
`,
    witness_sink: "print(result)",
    proof_relation: "the optimized sparse branch tree must select the value-matching arm and print thousand",
  }),
  descriptor({
    id: "mutant.observable-sink-removal",
    seam: "observable-sink removal",
    expected_layer: "conformance",
    source_file: "crates/jet-codegen/src/Codegen/TIR/emit/statements.rs",
    symbol: "emit_tir_stmt::TStmt::ExprStmt",
    selector: {
      node_kind: "call_expression",
      field: "expression",
      value: "emit_expr_with_cleanups(e, cx, active_deferred_closes)",
      pattern: "TStmt::ExprStmt(e)",
    },
    path: ["functions", "emit_tir_stmt", "expr_stmt_arm", "expression"],
    operation: "replace",
    replacement: { kind: "expression", source: "\"/* expression statement removed */\"" },
    witness_source: `fn run() {
    value :: 42
    print(value)
}
`,
    witness_sink: "print(value)",
    proof_relation: "the emitted print expression must remain an observable value-consuming sink and print 42",
  }),
]);

export function mutationCatalog() {
  return MUTATION_CATALOG.map((mutant) => clone(mutant));
}

export function validateMutationCatalog(catalog = MUTATION_CATALOG) {
  if (!Array.isArray(catalog) || catalog.length === 0) throw new Error("mutation catalog must not be empty");
  const seen = new Set();
  for (const mutant of catalog) {
    for (const field of REQUIRED_FIELDS) if (!Object.hasOwn(mutant, field)) throw new Error(`mutation catalog entry is missing ${field}`);
    if (seen.has(mutant.id)) throw new Error(`duplicate mutation catalog ID: ${mutant.id}`);
    seen.add(mutant.id);
    if (typeof mutant.id !== "string" || !mutant.id.startsWith("mutant.")) throw new Error(`mutation catalog ID is not stable: ${mutant.id}`);
    if (!mutant.ast_mutation || mutant.ast_mutation.language !== "rust") throw new Error(`mutation ${mutant.id} has no Rust AST operation`);
    if (typeof mutant.ast_mutation.source_file !== "string" || mutant.ast_mutation.source_file.length === 0) throw new Error(`mutation ${mutant.id} has no Rust source target`);
    if (typeof mutant.ast_mutation.symbol !== "string" || mutant.ast_mutation.symbol.length === 0) throw new Error(`mutation ${mutant.id} has no Rust symbol target`);
    if (!Array.isArray(mutant.ast_mutation.path) || mutant.ast_mutation.path.length === 0) throw new Error(`mutation ${mutant.id} has no AST path`);
    if (mutant.ast_mutation.path.some((part) => typeof part !== "string" || part.length === 0)) throw new Error(`mutation ${mutant.id} has an invalid AST path`);
    if (!Object.hasOwn(mutant.ast_mutation, "selector") || typeof mutant.ast_mutation.selector !== "object" || Array.isArray(mutant.ast_mutation.selector)) throw new Error(`mutation ${mutant.id} has no AST selector`);
    for (const field of ["node_kind", "field", "value", "pattern"]) {
      if (typeof mutant.ast_mutation.selector[field] !== "string" || mutant.ast_mutation.selector[field].length === 0) {
        throw new Error(`mutation ${mutant.id} has an inexact AST selector`);
      }
    }
    if (!["delete", "replace", "toggle"].includes(mutant.ast_mutation.operation)) throw new Error(`mutation ${mutant.id} has an invalid AST operation`);
    if (mutant.ast_mutation.operation === "replace" && mutant.ast_mutation.replacement == null) throw new Error(`mutation ${mutant.id} has no AST replacement`);
    if (mutant.ast_mutation.operation === "replace"
      && (typeof mutant.ast_mutation.replacement !== "object"
        || typeof mutant.ast_mutation.replacement.kind !== "string"
        || mutant.ast_mutation.replacement.kind.length === 0
        || typeof mutant.ast_mutation.replacement.source !== "string"
        || mutant.ast_mutation.replacement.source.length === 0)) {
      throw new Error(`mutation ${mutant.id} has an inexact AST replacement`);
    }
    if (!mutant.witness?.value_consuming || typeof mutant.witness.source !== "string" || !mutant.witness.source.includes("print(")) throw new Error(`mutation ${mutant.id} has no value-consuming witness`);
    if (typeof mutant.witness.sink !== "string" || mutant.witness.sink.length === 0) throw new Error(`mutation ${mutant.id} has no observable sink`);
    if (!mutant.witness.source.includes(mutant.witness.sink)) throw new Error(`mutation ${mutant.id} sink is not in its witness`);
    if (mutant.witness.normalization !== undefined
      && (!Array.isArray(mutant.witness.normalization)
        || mutant.witness.normalization.some((item) => typeof item !== "string" || item.length === 0))) {
      throw new Error(`mutation ${mutant.id} has invalid witness normalization`);
    }
    if (typeof mutant.proof_relation !== "string" || mutant.proof_relation.length === 0) throw new Error(`mutation ${mutant.id} has no proof relation`);
    if (typeof mutant.expected_layer !== "string" || !EXPECTED_KILLER_LAYERS.includes(mutant.expected_layer)) throw new Error(`mutation ${mutant.id} has no named expected killer`);
    if (typeof mutant.seam !== "string" || !CRITICAL_SILENT_DATA_SEAMS.includes(mutant.seam)) throw new Error(`mutation ${mutant.id} has an unreviewed seam`);
  }
  return true;
}

export function validateMustKillCatalog(catalog = MUTATION_CATALOG) {
  validateMutationCatalog(catalog);
  const covered = new Set(catalog.map((mutant) => mutant.seam));
  const missing = CRITICAL_SILENT_DATA_SEAMS.filter((seam) => !covered.has(seam));
  if (missing.length) throw new Error(`mutation catalog is missing critical seam(s): ${missing.join(", ")}`);
  return true;
}

function pathTarget(root, path) {
  let parent = root;
  for (let index = 0; index < path.length - 1; index += 1) {
    const key = path[index];
    if (parent === null || typeof parent !== "object" || !Object.hasOwn(parent, key)) throw new Error(`AST mutation path is missing: ${path.join(".")}`);
    parent = parent[key];
  }
  const key = path[path.length - 1];
  if (parent === null || typeof parent !== "object" || !Object.hasOwn(parent, key)) throw new Error(`AST mutation target is missing: ${path.join(".")}`);
  return { parent, key };
}

/** Apply one reviewed descriptor to an AST value. */
export function applyAstMutation(ast, mutant) {
  if (!ast || typeof ast !== "object") throw new Error("AST mutation input must be an object");
  if (!mutant?.ast_mutation) throw new Error("AST mutation descriptor is required");
  const edit = mutant.ast_mutation;
  const output = clone(ast);
  const { parent, key } = pathTarget(output, edit.path);
  if (edit.operation === "delete") {
    if (Array.isArray(parent)) {
      const index = Number(key);
      if (!Number.isInteger(index) || index < 0 || index >= parent.length) {
        throw new Error(`AST mutation array index is invalid: ${edit.path.join(".")}`);
      }
      parent.splice(index, 1);
    }
    else delete parent[key];
  } else if (edit.operation === "replace") {
    parent[key] = clone(edit.replacement);
  } else if (edit.operation === "toggle") {
    if (typeof parent[key] !== "boolean") throw new Error(`AST mutation toggle target is not boolean: ${mutant.id}`);
    parent[key] = !parent[key];
  } else {
    throw new Error(`unknown AST mutation operation: ${edit.operation}`);
  }
  return output;
}

export function mutateAstSource({ ast, mutant, print }) {
  if (typeof print !== "function") throw new Error("AST mutation requires an AST printer");
  const mutatedAst = applyAstMutation(ast, mutant);
  const source = print(mutatedAst);
  if (typeof source !== "string" || source.length === 0) throw new Error(`AST printer produced no source: ${mutant?.id}`);
  return { ast: mutatedAst, source, source_sha256: sha256(source) };
}

const RUST_MULTI_TOKENS = Object.freeze([
  "...", "..=", "::", "=>", "->", "==", "!=", "<=", ">=", "&&", "||", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<", ">>",
]);
const RUST_SYMBOL_WORDS = new Set(["as", "const", "crate", "else", "enum", "fn", "for", "if", "impl", "in", "let", "match", "mod", "move", "pub", "ref", "self", "struct", "trait", "type", "use", "where"]);

/**
 * Small lossless-enough Rust syntax tree. It deliberately records token spans
 * instead of replacing a matching source string: selectors identify one
 * lexical AST node, and edits use that node's byte range.
 */
function rustAstTokens(source) {
  const tokens = [];
  let index = 0;
  while (index < source.length) {
    const start = index;
    const character = source[index];
    if (/\s/.test(character)) {
      index += 1;
      continue;
    }
    if (source.startsWith("//", index)) {
      const newline = source.indexOf("\n", index + 2);
      index = newline < 0 ? source.length : newline;
      continue;
    }
    if (source.startsWith("/*", index)) {
      let depth = 1;
      index += 2;
      while (index < source.length && depth > 0) {
        if (source.startsWith("/*", index)) {
          depth += 1;
          index += 2;
        } else if (source.startsWith("*/", index)) {
          depth -= 1;
          index += 2;
        } else {
          index += 1;
        }
      }
      continue;
    }
    if (character === '"') {
      index += 1;
      while (index < source.length) {
        if (source[index] === "\\") index += 2;
        else if (source[index++] === '"') break;
      }
    } else if (character === "'") {
      // A lifetime (`'a`) has no closing quote. A character literal does.
      if (/[A-Za-z_]/.test(source[index + 1] || "") && source[index + 2] !== "'") {
        index += 2;
      } else {
        index += 1;
        while (index < source.length) {
          if (source[index] === "\\") index += 2;
          else if (source[index++] === "'") break;
        }
      }
    } else if (/[A-Za-z_$]/.test(character)) {
      index += 1;
      while (index < source.length && /[A-Za-z0-9_$]/.test(source[index])) index += 1;
    } else if (/[0-9]/.test(character)) {
      index += 1;
      while (index < source.length && /[A-Za-z0-9_]/.test(source[index])) index += 1;
    } else {
      const multi = RUST_MULTI_TOKENS.find((token) => source.startsWith(token, index));
      index += multi ? multi.length : 1;
    }
    tokens.push({ text: source.slice(start, index), start, end: index });
  }
  return tokens;
}

function rustTokenTexts(source) {
  return rustAstTokens(source).map((token) => token.text);
}

function tokenMatches(tokens, wanted) {
  if (!wanted.length || wanted.length > tokens.length) return [];
  const matches = [];
  for (let index = 0; index <= tokens.length - wanted.length; index += 1) {
    let equal = true;
    for (let offset = 0; offset < wanted.length; offset += 1) {
      if (tokens[index + offset].text !== wanted[offset]) {
        equal = false;
        break;
      }
    }
    if (equal) matches.push({
      token_start: index,
      token_end: index + wanted.length - 1,
      start: tokens[index].start,
      end: tokens[index + wanted.length - 1].end,
    });
  }
  return matches;
}

function rustSymbolWords(symbol) {
  return rustTokenTexts(symbol).filter((word) => /^[A-Za-z_$][A-Za-z0-9_$]*$/.test(word) && !RUST_SYMBOL_WORDS.has(word));
}

function symbolScore(tokens, candidate, symbol) {
  const words = rustSymbolWords(symbol);
  const start = Math.max(0, candidate.token_start - 500);
  const end = Math.min(tokens.length, candidate.token_end + 501);
  return words.filter((word) => tokens.slice(start, end).some((token) => token.text === word)).length;
}

function selectRustAstNode(source, mutant) {
  const edit = mutant?.ast_mutation;
  const selector = edit?.selector;
  if (!edit || !selector) throw new Error(`AST selector is missing: ${mutant?.id || "unknown"}`);
  const tokens = rustAstTokens(source);
  const valueTokens = rustTokenTexts(selector.value);
  const valueMatches = tokenMatches(tokens, valueTokens);
  if (valueMatches.length === 0) throw new Error(`AST selector value is missing: ${mutant.id}`);
  const patternTokens = rustTokenTexts(selector.pattern);
  const patternMatches = tokenMatches(tokens, patternTokens);
  const anchored = patternMatches.length === 0
    ? valueMatches
    : valueMatches.filter((value) => patternMatches.some((pattern) => (
      value.token_start >= pattern.token_start && value.token_end <= pattern.token_end
    )));
  const candidates = anchored.length ? anchored : valueMatches;
  const scored = candidates.map((candidate) => ({ candidate, score: symbolScore(tokens, candidate, edit.symbol) }));
  const highest = Math.max(...scored.map((item) => item.score));
  const best = scored.filter((item) => item.score === highest);
  if (best.length !== 1 || highest === 0) {
    throw new Error(`AST selector is not unique in ${edit.source_file}: ${mutant.id}`);
  }
  const selected = best[0].candidate;
  if (edit.node_kind === "string_literal" && (valueTokens.length !== 1 || !/^(["']).*\1$/.test(valueTokens[0]))) {
    throw new Error(`AST selector kind mismatch: ${mutant.id}`);
  }
  return {
    ...selected,
    node_kind: edit.node_kind,
    selector: clone(selector),
    matched: source.slice(selected.start, selected.end),
  };
}

function applyRustAstMutationToSource(source, mutant) {
  const text = Buffer.isBuffer(source) ? source.toString("utf8") : String(source);
  const tokens = rustAstTokens(text);
  const node = selectRustAstNode(text, mutant);
  const edit = mutant.ast_mutation;
  let start = node.start;
  let end = node.end;
  let replacement = "";
  if (edit.operation === "replace") {
    replacement = edit.replacement.source;
    if (!rustTokenTexts(replacement).length) throw new Error(`AST replacement is empty: ${mutant.id}`);
  } else if (edit.operation === "delete") {
    const next = tokens[node.token_end + 1];
    const previous = tokens[node.token_start - 1];
    if (next?.text === ",") end = next.end;
    else if (previous?.text === ",") start = previous.start;
  } else if (edit.operation !== "toggle") {
    throw new Error(`unknown Rust AST operation: ${edit.operation}`);
  }
  if (edit.operation === "toggle") throw new Error(`Rust AST toggle is unsupported without a boolean node: ${mutant.id}`);
  const mutated = `${text.slice(0, start)}${replacement}${text.slice(end)}`;
  if (mutated === text) throw new Error(`Rust AST mutation did not change source: ${mutant.id}`);
  const bytes = Buffer.from(mutated, "utf8");
  return {
    source: mutated,
    source_sha256: sha256(bytes),
    ast: {
      language: "rust",
      node,
      operation: edit.operation,
      source_file: edit.source_file,
      symbol: edit.symbol,
      path: [...edit.path],
    },
  };
}

/** Apply one catalog selector to real Rust source using token AST spans. */
export function applyRustAstMutation({ source, mutant }) {
  if (source === undefined || source === null) throw new Error("Rust AST mutation source is required");
  validateMutationCatalog([mutant]);
  return applyRustAstMutationToSource(source, mutant);
}

export const applyRustSourceMutation = applyRustAstMutation;

export function mutationCase(mutant, {
  seed = MUTATION_DEFAULT_SEED,
  baseline_source = mutant?.witness?.source || WITNESS_SOURCE,
} = {}) {
  validateMutationCatalog([mutant]);
  return {
    layer: "mutation",
    mutant_id: mutant.id,
    seam: mutant.seam,
    expected_layer: mutant.expected_layer,
    seed: `${seed}:${mutant.id}`,
    mutation_arm: `compiler:${mutant.id}`,
    mutator_version: MUTATION_SCHEMA,
    source: baseline_source,
    source_sha256: sha256(baseline_source),
    ast_mutation: clone(mutant.ast_mutation),
    witness: clone(mutant.witness),
    normalization: [...(mutant.witness.normalization || [])],
    proof_relation: mutant.proof_relation,
  };
}

function proofLayer(proof) {
  return proof?.layer || proof?.expected_layer || null;
}

function proofKilled(proof, expectedLayer = null) {
  if (!proof || typeof proof !== "object") return false;
  const status = typeof proof.status === "string" ? proof.status.toLowerCase() : "";
  if (["timeout", "timed-out", "timed_out", "signal", "crash", "crashed", "skipped", "build-failure", "build_failed", "survived", "not-killed"].includes(status)) return false;
  if (proof.timeout || proof.timed_out || proof.signal || proof.crashed || proof.build_failed || proof.skipped) return false;
  if (proof.exit !== undefined && proof.exit !== null && proof.exit !== 0) return false;
  if (proof.error !== undefined && proof.error !== null && String(proof.error).length > 0) return false;
  if (proof.value_consuming !== true) return false;
  if (proof.observable_mismatch !== true) return false;
  if (expectedLayer !== null && proofLayer(proof) !== expectedLayer) return false;
  return proof.killed === true || proof.status === "killed" || (proof.ok === false && proof.observable_mismatch === true);
}

function proofSummary(proof, expectedLayer = null, outcome = {}) {
  const proofConfigurationErrors = Array.isArray(proof?.configuration_errors) ? proof.configuration_errors : [];
  const outcomeConfigurationErrors = Array.isArray(outcome.configuration_errors) ? outcome.configuration_errors : [];
  const configurationErrors = [...new Set([...proofConfigurationErrors, ...outcomeConfigurationErrors])];
  if (!proof || typeof proof !== "object") return {
    status: "missing",
    value_consuming: false,
    observable_mismatch: false,
    layer: null,
    killed: false,
    timeout: false,
    signal: null,
    crashed: false,
    skipped: false,
    build_failed: false,
    time_to_kill_ms: null,
    evidence: null,
    ...outcome,
    configuration_errors: configurationErrors,
  };
  const status = typeof proof.status === "string" ? proof.status.toLowerCase() : "";
  const timeout = proof.timeout === true || proof.timed_out === true
    || ["timeout", "timed-out", "timed_out"].includes(status);
  const signal = proof.signal || (status === "signal" ? "unknown" : null);
  const crashed = proof.crashed === true || ["crash", "crashed"].includes(status);
  const skipped = proof.skipped === true || status === "skipped" || status === "disabled-killer";
  const buildFailed = proof.build_failed === true || ["build-failure", "build_failed"].includes(status);
  return {
    status: proof.status || (proofKilled(proof, expectedLayer) ? "killed" : "not-killed"),
    value_consuming: proof.value_consuming === true,
    observable_mismatch: proof.observable_mismatch === true,
    layer: proofLayer(proof),
    killed: proofKilled(proof, expectedLayer),
    timeout,
    signal,
    crashed,
    skipped,
    build_failed: buildFailed,
    time_to_kill_ms: Number.isFinite(outcome.time_to_kill_ms) ? outcome.time_to_kill_ms : null,
    evidence: proof.evidence || null,
    ...outcome,
    configuration_errors: configurationErrors,
  };
}

function immutable(value) {
  return freezeDeep(clone(value));
}

function mutationBundle(caseInput, proof, metadata = {}, outcome = {}) {
  const summary = proofSummary(proof, caseInput.expected_layer, outcome);
  const tier = ["aot", "jet_run", "interpreter"].includes(proof?.tier) ? proof.tier : "jet_run";
  const exit = Number.isInteger(proof?.exit)
    ? proof.exit
    : summary.timeout || summary.signal
      ? null
      : summary.crashed || summary.build_failed
        ? 1
        : 0;
  return makeResultBundle({
    run_id: metadata.run_id || "mutation-run",
    stable_surface_id: caseInput.mutant_id,
    tier,
    tier_command: typeof proof?.tier_command === "string" && proof.tier_command.length > 0
      ? proof.tier_command
      : `mutation:${caseInput.expected_layer}`,
    seed: caseInput.seed,
    mutation_arm: caseInput.mutation_arm,
    mutator_version: caseInput.mutator_version,
    source: caseInput.source,
    stdout: proof?.stdout ?? proof?.stdout_bytes ?? "",
    stderr: proof?.stderr ?? proof?.stderr_bytes ?? "",
    exit,
    signal: summary.signal,
    timeout: summary.timeout,
    expected_relation: caseInput.proof_relation,
    actual_relation: proof?.actual_relation || summary.status,
    normalization: caseInput.normalization || [],
    oracle: {
      name: `mutation:${caseInput.mutant_id}`,
      version: String(MUTATION_SCHEMA_VERSION),
      input_digest: sha256(stable({ mutant_id: caseInput.mutant_id, seed: caseInput.seed })),
      independence_class: "tier-self-diff",
      provenance: "hardening-mutation-layer-4",
    },
    commit: metadata.commit || "unknown-commit",
    binary_sha256: metadata.binary_sha256 || "sha256:unknown-binary",
    registry_snapshot_hash: metadata.registry_snapshot_hash || "sha256:unknown-registry",
    config_hash: metadata.config_hash || "sha256:unknown-config",
    classification: outcome.killed ? "hardening-mutation-killed" : "hardening-gap-survivor",
    tower_action: "create-or-update",
    tier_observations: [],
    applicable_tiers: null,
    layer: "mutation",
    mutant_id: caseInput.mutant_id,
    seam: caseInput.seam,
    expected_layer: caseInput.expected_layer,
    ast_mutation: caseInput.ast_mutation,
    mutated_source: caseInput.mutated_source,
    proof: summary,
  });
}

function gapCard(caseInput, bundle, proof, disabled) {
  const summary = proofSummary(proof, caseInput.expected_layer);
  const deduplicationKey = sha256(stable({
    mutant_id: caseInput.mutant_id,
    seam: caseInput.seam,
    expected_layer: caseInput.expected_layer,
    ast_mutation: caseInput.ast_mutation,
  }));
  const evidence = {
    mutant_id: caseInput.mutant_id,
    seam: caseInput.seam,
    expected_layer: caseInput.expected_layer,
    ast_mutation: clone(caseInput.ast_mutation),
    missing_proof: {
      reason: disabled ? "expected killer disabled by negative control" : summary.status,
      layer: summary.layer,
      status: summary.status,
      value_consuming: summary.value_consuming,
      observable_mismatch: summary.observable_mismatch,
      time_to_kill_ms: summary.time_to_kill_ms,
      evidence: summary.evidence,
    },
    expected_relation: caseInput.proof_relation,
    mutated_source_sha256: bundle.mutated_source_sha256 || null,
    bundle_identity: sha256(stable(bundle)),
  };
  return {
    identity: deduplicationKey,
    deduplication_key: deduplicationKey,
    title: `Hardening gap: ${caseInput.mutant_id}`,
    reason: "catalog mutant survived without a value-consuming rejecting observation",
    payload: evidence,
  };
}

function omittedGapCard(mutant, seed) {
  const input = mutationCase(mutant, { seed });
  const deduplicationKey = sha256(stable({
    mutant_id: input.mutant_id,
    seam: input.seam,
    expected_layer: input.expected_layer,
    ast_mutation: input.ast_mutation,
  }));
  return {
    identity: deduplicationKey,
    deduplication_key: deduplicationKey,
    title: `Hardening gap: ${input.mutant_id}`,
    reason: "catalog mutant was not attempted; mutation denominator is incomplete",
    payload: {
      mutant_id: input.mutant_id,
      seam: input.seam,
      expected_layer: input.expected_layer,
      ast_mutation: clone(input.ast_mutation),
      missing_proof: {
        reason: "catalog entry omitted by mutation attempt bound",
        layer: input.expected_layer,
        status: "not-attempted",
        value_consuming: false,
        observable_mismatch: false,
        time_to_kill_ms: null,
        evidence: null,
      },
      bundle_identity: null,
    },
  };
}

function baselineCheck(expected, actual) {
  if (!expected || !actual) return false;
  if (expected.source_sha256 !== actual.source_sha256 || expected.target_sha256 !== actual.target_sha256) return false;
  return expected.commit === undefined || expected.commit === actual.commit;
}

function catalogProjection(catalog) {
  return Array.isArray(catalog)
    ? catalog.map((mutant) => ({
      id: mutant?.id,
      seam: mutant?.seam,
      expected_layer: mutant?.expected_layer,
      ast_mutation: clone(mutant?.ast_mutation),
      witness: clone(mutant?.witness),
      proof_relation: mutant?.proof_relation,
    })).sort((left, right) => String(left.id).localeCompare(String(right.id)))
    : [];
}

function reviewedCatalogReasons(catalog) {
  const reasons = [];
  try {
    validateMustKillCatalog(catalog);
  } catch (error) {
    reasons.push(error.message);
  }
  const expected = catalogProjection(MUTATION_CATALOG);
  const actual = catalogProjection(catalog);
  if (stable(expected) !== stable(actual)) reasons.push("mutation catalog denominator is not the reviewed catalog");
  return reasons;
}

export function deriveMutationScore(catalog, bundles) {
  const reasons = reviewedCatalogReasons(catalog);
  const rows = [];
  const byId = new Map();
  if (!Array.isArray(bundles)) {
    reasons.push("mutation result bundles are missing");
  } else {
    if (!Object.isFrozen(bundles)) reasons.push("mutation result bundle list is mutable");
    for (const bundle of bundles) {
      if (!bundle || typeof bundle !== "object") {
        reasons.push("mutation result bundle is not an object");
        continue;
      }
      if (!Object.isFrozen(bundle) || !Object.isFrozen(bundle.proof) || !Object.isFrozen(bundle.ast_mutation)) {
        reasons.push(`mutation result bundle is not immutable: ${bundle.mutant_id || "unknown"}`);
      }
      if (byId.has(bundle.mutant_id)) reasons.push(`duplicate mutation result bundle: ${bundle.mutant_id}`);
      else byId.set(bundle.mutant_id, bundle);
    }
  }
  for (const mutant of Array.isArray(catalog) ? catalog : []) {
    const bundle = byId.get(mutant.id);
    let killed = false;
    if (!bundle) {
      reasons.push(`missing mutation result bundle: ${mutant.id}`);
    } else {
      if (bundle.seam !== mutant.seam || bundle.expected_layer !== mutant.expected_layer) {
        reasons.push(`mutation result metadata mismatch: ${mutant.id}`);
      }
      if (stable(bundle.ast_mutation) !== stable(mutant.ast_mutation)) {
        reasons.push(`mutation AST descriptor mismatch: ${mutant.id}`);
      }
      const proof = bundle.proof;
      if (proof?.baseline_restored !== true) reasons.push(`mutation baseline was not restored: ${mutant.id}`);
      if (proof?.workspace_removed !== true) reasons.push(`mutation workspace was not removed: ${mutant.id}`);
      if (proof?.configuration_errors?.length) reasons.push(`mutation configuration is invalid: ${mutant.id}`);
      if (typeof bundle.mutated_source !== "string" || bundle.mutated_source === bundle.source) {
        reasons.push(`mutation did not change source: ${mutant.id}`);
      }
      if (bundle.source_sha256 !== sha256(bundle.source)
        || bundle.mutated_source_sha256 !== sha256(bundle.mutated_source || "")) {
        reasons.push(`mutation bundle digest mismatch: ${mutant.id}`);
      }
      if (bundle.commit !== "unknown-commit") {
        if (proof?.expected_commit !== bundle.commit || proof?.current_commit !== bundle.commit) {
          reasons.push(`mutation target commit is stale: ${mutant.id}`);
        }
      }
      if (bundle.binary_sha256 !== "sha256:unknown-binary") {
        if (proof?.expected_target_sha256 !== bundle.binary_sha256 || proof?.current_target_sha256 !== bundle.binary_sha256) {
          reasons.push(`mutation target binary is stale: ${mutant.id}`);
        }
      }
      if (!proof?.expected_source_sha256 || proof.current_source_sha256 !== proof.expected_source_sha256) {
        reasons.push(`mutation source baseline is stale: ${mutant.id}`);
      }
      killed = proofKilled(proof, mutant.expected_layer)
        && proof.baseline_restored === true
        && proof.workspace_removed === true
        && bundle.classification === "hardening-mutation-killed"
        && !reasons.some((reason) => reason.endsWith(`: ${mutant.id}`));
      if (!killed && bundle.classification === "hardening-mutation-killed") {
        reasons.push(`mutation was marked killed without valid proof: ${mutant.id}`);
      }
    }
    rows.push({ mutant_id: mutant.id, killed, bundle: bundle || null });
  }
  if (byId.size !== (Array.isArray(catalog) ? catalog.length : 0)) reasons.push("mutation result bundle denominator mismatch");
  const survivors = rows.filter((row) => !row.killed);
  const total = rows.length;
  if (survivors.length) reasons.push(`mutation survivors: ${survivors.map((row) => row.mutant_id).join(", ")}`);
  return {
    status: reasons.length || survivors.length ? "RED" : "PASS",
    total,
    killed: rows.filter((row) => row.killed).length,
    survivors: survivors.length,
    score: total ? rows.filter((row) => row.killed).length / total : 0,
    survivor_ids: survivors.map((row) => row.mutant_id),
    red_reasons: [...new Set(reasons)],
  };
}

/**
 * Run catalog entries strictly serially.  Callback names are intentionally
 * narrow so the existing rig can connect its lease/build/proof/cleanup seams.
 */
export async function runMutationSensitivity({
  catalog = MUTATION_CATALOG,
  seed = MUTATION_DEFAULT_SEED,
  maxMutants = MUTATION_MAX_CASES,
  lease = null,
  baseline,
  apply,
  build,
  prove,
  restore,
  removeWorkspace = null,
  workspaceRequired = false,
  onMutantStart = null,
  onMutantEnd = null,
  metadata = {},
  disabledKillers = [],
  manualExemptions = [],
  exemptions = [],
  ...unsupportedOptions
} = {}) {
  validateMutationCatalog(catalog);
  if (!Number.isInteger(maxMutants) || maxMutants < 1 || maxMutants > MUTATION_MAX_CASES) throw new Error(`mutation maxMutants must be an integer from 1 through ${MUTATION_MAX_CASES}`);
  if (!baseline || typeof baseline.source_sha256 !== "string" || typeof baseline.target_sha256 !== "string") throw new Error("mutation baseline source and target checksums are required");
  for (const [name, callback] of Object.entries({ apply, build, prove, restore })) if (typeof callback !== "function") throw new Error(`mutation ${name} callback is required`);
  if (!Array.isArray(disabledKillers)) throw new Error("mutation disabledKillers must be an array");
  if (!Array.isArray(manualExemptions) || !Array.isArray(exemptions)) throw new Error("mutation exemptions must be arrays");
  if (typeof workspaceRequired !== "boolean") throw new Error("mutation workspaceRequired must be boolean");
  if (workspaceRequired && typeof removeWorkspace !== "function") {
    throw new Error("mutation removeWorkspace callback is required when workspaceRequired is true");
  }
  if (workspaceRequired && typeof baseline.current !== "function") {
    throw new Error("mutation baseline.current checksum callback is required when workspaceRequired is true");
  }
  if (workspaceRequired && baseline.target_sha256 === "sha256:unknown-binary") {
    throw new Error("mutation target/debug/jet checksum is required when workspaceRequired is true");
  }
  for (const [name, callback] of Object.entries({ onMutantStart, onMutantEnd })) {
    if (callback !== null && typeof callback !== "function") throw new Error(`mutation ${name} callback must be a function`);
  }
  const expectedBaseline = { ...baseline };
  if (expectedBaseline.commit === undefined && typeof metadata.commit === "string") expectedBaseline.commit = metadata.commit;
  const configurationErrors = Object.keys(unsupportedOptions)
    .sort()
    .map((key) => `unsupported mutation option: ${key}`);
  if (manualExemptions.length || exemptions.length) {
    configurationErrors.push("manual mutation exemptions are forbidden");
  }
  const knownTargets = new Set(catalog.flatMap((mutant) => [mutant.id, mutant.expected_layer]));
  for (const target of [...disabledKillers, ...manualExemptions, ...exemptions]) {
    if (typeof target !== "string" || !knownTargets.has(target)) {
      configurationErrors.push(`mutation exemption target is not in the catalog: ${String(target)}`);
    }
  }
  const disabled = new Set([...disabledKillers, ...manualExemptions, ...exemptions]);
  const killerDisabled = (mutant) => disabled.has(mutant.id) || disabled.has(mutant.expected_layer);
  const results = [];
  const bundles = [];
  const gaps = new Map();
  let active = 0;
  let leaseHeld = false;
  try {
    if (lease?.acquire) {
      await lease.acquire();
      leaseHeld = true;
    }
    for (const mutant of catalog.slice(0, maxMutants)) {
      if (active !== 0) throw new Error("mutation catalog is not serial");
      active += 1;
      try {
        const input = mutationCase(mutant, {
          seed,
          baseline_source: typeof expectedBaseline.source === "string"
            ? expectedBaseline.source
            : mutant.witness.source,
        });
        let buildResult = null;
        let proof = null;
        let restored = false;
        let workspaceRemoved = !workspaceRequired && removeWorkspace === null;
        let restoreInfo = null;
        let error = null;
        const startedAt = Date.now();
        let timeToKillMs = null;
        try {
          if (onMutantStart) await onMutantStart(mutant, input);
          const applied = await apply(mutant, input);
          if (!applied || typeof applied.source !== "string" || applied.source.length === 0) {
            throw new Error(`AST mutation adapter returned no mutated source: ${mutant.id}`);
          }
          const mutatedSourceSha256 = sha256(applied.source);
          if (mutatedSourceSha256 === input.source_sha256) {
            throw new Error(`AST mutation did not change source: ${mutant.id}`);
          }
          if (applied.source_sha256 !== undefined && applied.source_sha256 !== mutatedSourceSha256) {
            throw new Error(`AST mutation returned a stale source checksum: ${mutant.id}`);
          }
          input.mutated_source = applied.source;
          buildResult = await build(mutant, input);
          if (!buildResult?.ok) {
            const timedOut = buildResult?.timed_out === true || buildResult?.timeout === true;
            proof = {
              status: timedOut ? "timeout" : "build-failure",
              layer: mutant.expected_layer,
              build_failed: true,
              timeout: timedOut,
              timed_out: timedOut,
              signal: buildResult?.signal || null,
              exit: buildResult?.exit ?? null,
              value_consuming: false,
              evidence: buildResult?.evidence || null,
            };
          } else {
            proof = killerDisabled(mutant)
              ? {
                status: "disabled-killer",
                layer: mutant.expected_layer,
                value_consuming: false,
                skipped: true,
              }
              : await prove(mutant.expected_layer, mutant, input);
            if (proofKilled(proof, mutant.expected_layer)) timeToKillMs = Math.max(0, Date.now() - startedAt);
          }
        } catch (failure) {
          error = failure;
          proof = {
            status: "mutation-failure",
            layer: mutant.expected_layer,
            value_consuming: false,
            crashed: true,
            evidence: failure?.message || String(failure),
          };
        } finally {
          try {
            restoreInfo = await restore(mutant, input, expectedBaseline);
            restored = true;
          } catch (failure) {
            error ||= failure;
          }
          if (removeWorkspace) {
            try {
              await removeWorkspace(mutant, input);
              workspaceRemoved = true;
            } catch (failure) {
              error ||= failure;
            }
          }
          if (onMutantEnd) {
            try {
              await onMutantEnd(mutant, input);
            } catch (failure) {
              error ||= failure;
            }
          }
        }
        let after = null;
        try {
          after = typeof expectedBaseline.current === "function"
            ? await expectedBaseline.current(mutant, input)
            : expectedBaseline;
        } catch (failure) {
          error ||= failure;
        }
        const baseline_restored = restored && baselineCheck(expectedBaseline, after);
        const outcome = {
          baseline_restored,
          workspace_removed: workspaceRemoved,
          expected_source_sha256: expectedBaseline.source_sha256,
          current_source_sha256: after?.source_sha256 || null,
          expected_target_sha256: expectedBaseline.target_sha256,
          current_target_sha256: after?.target_sha256 || null,
          expected_commit: expectedBaseline.commit || null,
          current_commit: after?.commit || null,
          time_to_kill_ms: timeToKillMs,
          workspace_required: workspaceRequired,
          serial: true,
          configuration_errors: [...configurationErrors],
          ...(restoreInfo && typeof restoreInfo === "object" ? restoreInfo : {}),
        };
        const killed = proofKilled(proof, mutant.expected_layer) && baseline_restored && workspaceRemoved;
        const bundle = immutable(mutationBundle(
          input,
          proof,
          {
            ...metadata,
            commit: metadata.commit ?? expectedBaseline.commit,
            binary_sha256: metadata.binary_sha256 ?? expectedBaseline.target_sha256,
          },
          { ...outcome, killed },
        ));
        bundles.push(bundle);
        const row = immutable({
          mutant_id: mutant.id,
          seam: mutant.seam,
          expected_layer: mutant.expected_layer,
          status: killed ? "KILLED" : "SURVIVED",
          killed,
          proof: bundle.proof,
          build: buildResult,
          source_sha256: input.source_sha256,
          mutated_source_sha256: bundle.mutated_source_sha256 || null,
          baseline_restored,
          workspace_removed: workspaceRemoved,
          error: error?.message || null,
        });
        results.push(row);
        if (!killed) {
          const card = gapCard(input, bundle, proof, killerDisabled(mutant));
          if (!gaps.has(card.identity)) gaps.set(card.identity, immutable(card));
        }
      } finally {
        active -= 1;
      }
    }
  } finally {
    active = 0;
    if (leaseHeld && lease?.release) await lease.release();
  }
  for (const mutant of catalog.slice(maxMutants)) {
    const card = omittedGapCard(mutant, seed);
    if (!gaps.has(card.identity)) gaps.set(card.identity, immutable(card));
  }
  const catalogSnapshot = immutable(catalog.map((mutant) => ({
    id: mutant.id,
    seam: mutant.seam,
    expected_layer: mutant.expected_layer,
    ast_mutation: clone(mutant.ast_mutation),
    witness: clone(mutant.witness),
    proof_relation: mutant.proof_relation,
  })));
  const immutableBundles = Object.freeze(bundles);
  const score = deriveMutationScore(catalogSnapshot, immutableBundles);
  const serialized_bundles = serializeBundles(immutableBundles);
  const survivorBundles = Object.freeze(
    immutableBundles.filter((bundle) => score.survivor_ids.includes(bundle.mutant_id)),
  );
  const omitted_mutant_ids = catalog.slice(maxMutants).map((mutant) => mutant.id);
  return {
    schema: MUTATION_SCHEMA,
    schema_version: MUTATION_SCHEMA_VERSION,
    seed: String(seed),
    status: score.status,
    catalog: catalogSnapshot,
    attempted: results.length,
    killed: score.killed,
    survivors: score.survivors,
    mutation_score: score.score,
    survivor_ids: score.survivor_ids,
    omitted_mutant_ids: Object.freeze(omitted_mutant_ids),
    red_reasons: score.red_reasons,
    results: Object.freeze(results),
    bundles: immutableBundles,
    findings: survivorBundles,
    serialized_bundles,
    bundle_sha256: sha256(serialized_bundles),
    gap_cards: immutable([...gaps.values()]),
  };
}

export const runMutationLayer = runMutationSensitivity;

export function mutationScore(summary) {
  if (!summary || !Array.isArray(summary.catalog)) {
    return {
      status: "RED",
      total: 0,
      killed: 0,
      survivors: 0,
      score: 0,
      survivor_ids: [],
      red_reasons: ["mutation score catalog is missing"],
    };
  }
  return deriveMutationScore(summary.catalog, summary.bundles);
}

export function checkMutationCatalogShape() {
  validateMustKillCatalog();
  const ast = {
    functions: {
      values_equal: {
        list_arm: {
          expression: { kind: "expression", source: "list_values_equal(left, right)" },
        },
      },
    },
  };
  const mutated = applyAstMutation(ast, MUTATION_CATALOG[0]);
  if (mutated.functions.values_equal.list_arm.expression.source !== "left.len() == right.len()") {
    throw new Error("replace AST mutation did not apply");
  }
  return mutationCatalog();
}

export const MUTATION_ADAPTER_CONTRACT = Object.freeze({
  schema: "jet.hardening.mutation-adapter.v1",
  methods: Object.freeze(["current", "apply", "build", "prove", "restore", "removeWorkspace", "interrupt"]),
  guarantees: Object.freeze(["disposable-worktree", "byte-exact-restore", "process-group-cleanup", "bounded-build"]),
});

const REAL_ADAPTER_SCHEMA = MUTATION_ADAPTER_CONTRACT.schema;
const REAL_ADAPTER_ROOT = resolve(fileURLToPath(new URL("../..", import.meta.url)));
const REAL_ADAPTER_WORKTREE_ROOT = ".claude/worktrees";
const REAL_ADAPTER_MAX_CAPTURE = 256 * 1024;
const REAL_ADAPTER_TARGET_CAP_GIB = 80;
const REAL_ADAPTER_SERVICE_MEMORY_GIB = 8;
const REAL_ADAPTER_BUILD_JOBS = 4;
const REAL_ADAPTER_DEFAULT_TIMEOUT_MS = 95 * 60 * 1000;
const REAL_ADAPTER_GIB = 1024 ** 3;
const REAL_ADAPTER_BUILD_PATHS = Object.freeze(["Cargo.toml", "Cargo.lock", "Source", "crates", "corelib"]);

function adapterDelay(milliseconds) {
  return new Promise((resolvePromise) => setTimeout(resolvePromise, milliseconds));
}

function adapterProcessAlive(pid) {
  if (!Number.isInteger(pid) || pid <= 1) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

function adapterPathWithin(parent, child) {
  const path = relative(resolve(parent), resolve(child));
  return path !== "" && path !== ".." && !path.startsWith("../") && !path.startsWith("..\\") && !isAbsolute(path);
}

function adapterFileSha256(path) {
  try {
    return sha256(readFileSync(path));
  } catch {
    return "sha256:missing";
  }
}

function adapterGitHead(root) {
  const result = spawnSync("git", ["-C", root, "rev-parse", "HEAD"], { encoding: "utf8" });
  if (result.status !== 0) throw new Error(`mutation git identity failed: ${String(result.stderr || "").trim()}`);
  return String(result.stdout).trim();
}

function adapterGitWorktree(root, args) {
  const result = spawnSync("git", ["-C", root, "worktree", ...args], { encoding: "utf8" });
  if (result.status !== 0) {
    throw new Error(`mutation worktree command failed: ${String(result.stderr || result.stdout || "").trim()}`);
  }
  return result;
}

function adapterMirrorBuildInputs(state) {
  const diff = spawnSync("git", ["-C", state.root, "diff", "--binary", "HEAD", "--", ...REAL_ADAPTER_BUILD_PATHS], {
    encoding: null,
    maxBuffer: 128 * 1024 * 1024,
  });
  if (diff.status !== 0) throw new Error(`mutation source diff failed: ${String(diff.stderr || "").trim()}`);
  if (diff.stdout?.length) {
    const applied = spawnSync("git", ["-C", state.worktreePath, "apply", "--binary", "--whitespace=nowarn", "-"], {
      input: diff.stdout,
      encoding: "utf8",
      maxBuffer: 16 * 1024 * 1024,
    });
    if (applied.status !== 0) throw new Error(`mutation source diff apply failed: ${String(applied.stderr || applied.stdout || "").trim()}`);
  }
  const untracked = spawnSync("git", ["-C", state.root, "ls-files", "--others", "--exclude-standard", "-z", "--", ...REAL_ADAPTER_BUILD_PATHS], {
    encoding: null,
    maxBuffer: 128 * 1024 * 1024,
  });
  if (untracked.status !== 0) throw new Error(`mutation untracked source scan failed: ${String(untracked.stderr || "").trim()}`);
  for (const name of Buffer.from(untracked.stdout || "").toString("utf8").split("\0").filter(Boolean)) {
    const source = resolve(state.root, name);
    const destination = resolve(state.worktreePath, name);
    if (!adapterPathWithin(state.root, source) || !adapterPathWithin(state.worktreePath, destination)) {
      throw new Error(`mutation source mirror escapes checkout: ${name}`);
    }
    mkdirSync(dirname(destination), { recursive: true, mode: 0o700 });
    writeFileSync(destination, readFileSync(source));
  }
}

function adapterCheckoutSource(root, sourceFile) {
  const normalized = String(sourceFile).replaceAll("\\", "/");
  const path = resolve(root, normalized);
  if (!adapterPathWithin(root, path)) throw new Error(`mutation source escapes checkout: ${sourceFile}`);
  try {
    return readFileSync(path);
  } catch (error) {
    throw new Error(`mutation source snapshot failed: ${error.message}`);
  }
}

function adapterWriteDurable(path, bytes) {
  writeFileSync(path, bytes);
  const descriptor = openSync(path, "r");
  try {
    fsyncSync(descriptor);
  } finally {
    closeSync(descriptor);
  }
}

function adapterReadJson(path) {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return null;
  }
}

function adapterDiskBytes(path) {
  if (!existsSync(path)) return 0;
  const result = spawnSync("du", ["-s", "-B1", "--", path], { encoding: "utf8" });
  if (result.status !== 0) return null;
  const match = String(result.stdout).match(/^(\d+)/);
  return match ? Number(match[1]) : null;
}

function adapterCapture(current, chunk) {
  const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
  if (current.length >= REAL_ADAPTER_MAX_CAPTURE) return current;
  return Buffer.concat([current, bytes.subarray(0, REAL_ADAPTER_MAX_CAPTURE - current.length)]);
}

function adapterLimitedCommand(program, args, memoryGib) {
  if (process.platform !== "linux") return { program, args };
  const prlimit = ["/usr/bin/prlimit", "/bin/prlimit"].find((candidate) => existsSync(candidate));
  if (prlimit) return { program: prlimit, args: [`--as=${memoryGib * REAL_ADAPTER_GIB}`, "--", program, ...args] };
  const shell = ["/bin/bash", "/usr/bin/bash"].find((candidate) => existsSync(candidate));
  if (!shell) return { program, args };
  return {
    program: shell,
    args: ["-c", `ulimit -v ${memoryGib * 1024 * 1024}; exec "$@"`, "jet-mutation-memory", program, ...args],
  };
}

async function adapterTerminate(record, force = false) {
  if (!record?.pid || record.pid <= 1) return;
  const signal = force ? "SIGKILL" : "SIGTERM";
  try {
    if (process.platform === "win32") record.child.kill(signal);
    else process.kill(-record.pid, signal);
  } catch {
    // The process group can exit between observation and signaling.
  }
  if (!force) {
    await adapterDelay(100);
    await adapterTerminate(record, true);
  }
}

function adapterRunProcess(state, program, args, {
  cwd,
  env,
  stdin,
  timeoutMs,
  label,
} = {}) {
  const limited = adapterLimitedCommand(program, args, state.serviceMemoryGib);
  const input = stdin === undefined || stdin === null ? null : Buffer.from(String(stdin), "utf8");
  const child = spawn(limited.program, limited.args, {
    cwd,
    detached: process.platform !== "win32",
    env: { ...process.env, ...env },
    stdio: [input === null ? "ignore" : "pipe", "pipe", "pipe"],
  });
  const record = {
    label,
    command: [limited.program, ...limited.args].join(" "),
    child,
    pid: child.pid || null,
    stdout: Buffer.alloc(0),
    stderr: Buffer.alloc(0),
    timeout: false,
    signal: null,
    exit: null,
    error: null,
  };
  state.children.add(record);
  child.stdout?.on("data", (chunk) => { record.stdout = adapterCapture(record.stdout, chunk); });
  child.stderr?.on("data", (chunk) => { record.stderr = adapterCapture(record.stderr, chunk); });
  child.stdout?.on("error", () => {});
  child.stderr?.on("error", () => {});
  child.stdin?.on("error", () => {});
  if (input !== null) child.stdin?.end(input);
  let termination = null;
  const timer = setTimeout(() => {
    record.timeout = true;
    termination = adapterTerminate(record);
  }, timeoutMs);
  return new Promise((resolvePromise) => {
    let closed = false;
    child.once("error", (error) => {
      record.error = error.message;
      closed = true;
    });
    child.once("close", (exit, signal) => {
      record.exit = exit ?? null;
      record.signal = signal || null;
      closed = true;
    });
    void (async () => {
      while (!closed) {
        if (state.interrupted && !termination) {
          termination = adapterTerminate(record);
        }
        await adapterDelay(25);
      }
      clearTimeout(timer);
      if (termination) await termination;
      // A successful wrapper can leave a descendant alive. Kill only this
      // command's detached process group before releasing the worktree.
      await adapterTerminate(record, true);
      state.children.delete(record);
      resolvePromise({
        ...record,
        stdout_bytes: record.stdout.length,
        stderr_bytes: record.stderr.length,
        stdout_sha256: sha256(record.stdout),
        stderr_sha256: sha256(record.stderr),
        timed_out: record.timeout,
        ok: !record.error && !record.timeout && record.exit === 0 && !record.signal,
      });
    })();
  });
}

function adapterEnvironment(state, context, root, targetDir) {
  return {
    NO_COLOR: "1",
    CARGO_BUILD_JOBS: String(REAL_ADAPTER_BUILD_JOBS),
    CARGO_INCREMENTAL: "0",
    CARGO_TARGET_DIR: targetDir,
    JET_TARGET_CAP_GB: String(REAL_ADAPTER_TARGET_CAP_GIB),
    JET_MIN_FREE_GB: "16",
    JET_TEST_SCRATCH: context.scratch,
    JET_TEST_SCRATCH_DIR: context.scratch,
    JET_DEV_ORACLE_CACHE_DIR: join(context.scratch, "oracle-cache"),
    TMPDIR: context.scratch,
    TMP: context.scratch,
    TEMP: context.scratch,
    JET_HARDENING_ROOT: root,
    JET_HARDENING_MUTANT: context.mutant.id,
    JET_HARDENING_SERVICE_MEMORY_GIB: String(state.serviceMemoryGib),
    // The required in-repo worktree path can exceed sccache's Unix socket
    // name limit. The isolated target remains bounded; bypass only sccache.
    JET_NO_SCCACHE: "1",
  };
}

function adapterJetEnv(state, root) {
  return state.jetEnv || join(root, "scripts/agent/jet-env");
}

function adapterTiers(layer) {
  if (layer === "differential") return ["aot", "jet_run", "interpreter"];
  if (layer === "grammar") return ["aot"];
  return ["jet_run"];
}

async function adapterRunTier(state, context, root, targetDir, tier) {
  const command = tierCommand(tier, context.witnessPath, {
    root,
    jetEnv: adapterJetEnv(state, root),
  });
  const result = await adapterRunProcess(state, command.program, command.args, {
    cwd: root,
    env: adapterEnvironment(state, context, root, targetDir),
    timeoutMs: state.timeoutMs,
    label: `mutation:${context.mutant.id}:${tier}`,
  });
  return { ...result, tier, tier_command: command.tier_command };
}

function adapterObservationFailure(observation) {
  if (!observation) return "missing";
  if (observation.timed_out) return "timeout";
  if (observation.signal) return "signal";
  if (observation.error || observation.exit !== 0 || !observation.ok) return "crash";
  return null;
}

function adapterObservedValue(observation) {
  return observation?.stdout?.toString("utf8").replaceAll("\r\n", "\n").trimEnd() || "";
}

function adapterValueConsuming(context, observation) {
  return context.mutant.witness.value_consuming === true
    && context.mutant.witness.source.includes(context.mutant.witness.sink)
    && Buffer.isBuffer(observation?.stdout)
    && observation.stdout.length > 0;
}

function adapterProofFailure(layer, observation, evidence) {
  const status = adapterObservationFailure(observation) || "not-killed";
  return {
    layer,
    tier: observation?.tier || "jet_run",
    tier_command: observation?.tier_command,
    status,
    killed: false,
    value_consuming: false,
    observable_mismatch: false,
    timeout: status === "timeout",
    timed_out: status === "timeout",
    signal: observation?.signal || (status === "signal" ? "unknown" : null),
    crashed: status === "crash",
    error: status,
    exit: observation?.exit ?? null,
    stdout: observation?.stdout || "",
    stderr: observation?.stderr || "",
    evidence,
  };
}

function adapterProof(context, layer, observations) {
  const tiers = adapterTiers(layer);
  for (const tier of tiers) {
    const baseline = context.baselineObservations[tier];
    const mutated = observations[tier];
    const failure = adapterObservationFailure(baseline) || adapterObservationFailure(mutated);
    if (failure) {
      return adapterProofFailure(layer, mutated || baseline, {
        failure,
        baseline: baseline ? adapterObservedValue(baseline) : null,
        mutated: mutated ? adapterObservedValue(mutated) : null,
        compiler_source_before_sha256: context.sourceBeforeSha256,
        compiler_source_mutated_sha256: context.sourceMutatedSha256,
      });
    }
  }
  const selected = observations[tiers[0]];
  const valueConsuming = tiers.every((tier) => adapterValueConsuming(context, context.baselineObservations[tier])
    && adapterValueConsuming(context, observations[tier]));
  const mismatches = tiers.filter((tier) => (
    adapterObservedValue(context.baselineObservations[tier]) !== adapterObservedValue(observations[tier])
  ));
  const observableMismatch = valueConsuming && mismatches.length > 0;
  return {
    layer,
    tier: selected.tier,
    tier_command: selected.tier_command,
    status: observableMismatch ? "killed" : "survived",
    killed: observableMismatch,
    ok: true,
    value_consuming: valueConsuming,
    observable_mismatch: observableMismatch,
    timeout: false,
    signal: null,
    crashed: false,
    exit: selected.exit,
    stdout: selected.stdout,
    stderr: selected.stderr,
    actual_relation: adapterObservedValue(selected),
    evidence: {
      before: adapterObservedValue(context.baselineObservations[tiers[0]]),
      after: adapterObservedValue(selected),
      mismatched_tiers: mismatches,
      value_consuming: valueConsuming,
      compiler_source_before_sha256: context.sourceBeforeSha256,
      compiler_source_mutated_sha256: context.sourceMutatedSha256,
    },
  };
}

function adapterLockPath(state) {
  return join(state.worktreeRoot, ".mutation-adapter.lock");
}

function adapterAcquireLock(state) {
  const path = adapterLockPath(state);
  mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
  try {
    mkdirSync(path, { mode: 0o700 });
  } catch (error) {
    if (error.code !== "EEXIST") throw error;
    const owner = adapterReadJson(join(path, "owner.json"));
    if (owner?.pid && adapterProcessAlive(owner.pid)) throw new Error("mutation adapter is already active");
    rmSync(path, { recursive: true, force: true });
    mkdirSync(path, { mode: 0o700 });
  }
  writeFileSync(join(path, "owner.json"), `${JSON.stringify({ pid: process.pid, mutant: state.mutantId })}\n`, { mode: 0o600 });
  state.lockHeld = true;
}

function adapterReleaseLock(state) {
  if (!state.lockHeld) return;
  rmSync(adapterLockPath(state), { recursive: true, force: true });
  state.lockHeld = false;
}

function adapterBuildLeasePath(state) {
  return join(state.root, "target", ".jet-hardening-build.lock");
}

function adapterAcquireBuildLease(state) {
  const path = adapterBuildLeasePath(state);
  mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
  try {
    mkdirSync(path, { mode: 0o700 });
  } catch (error) {
    if (error.code !== "EEXIST") throw error;
    const owner = adapterReadJson(join(path, "owner.json"));
    if (owner?.pid === process.pid) {
      state.buildLeasePath = path;
      state.buildLeaseBorrowed = true;
      return;
    }
    if (!owner?.pid || adapterProcessAlive(owner.pid)) throw new Error("mutation build lease is already active");
    rmSync(path, { recursive: true, force: true });
    mkdirSync(path, { mode: 0o700 });
  }
  writeFileSync(join(path, "owner.json"), `${JSON.stringify({
    schema: REAL_ADAPTER_SCHEMA,
    pid: process.pid,
    mutant: state.mutantId,
  })}\n`, { mode: 0o600 });
  state.buildLeasePath = path;
  state.buildLeaseOwned = true;
}

function adapterReleaseBuildLease(state) {
  if (!state.buildLeasePath || !state.buildLeaseOwned) {
    state.buildLeasePath = null;
    state.buildLeaseBorrowed = false;
    return;
  }
  const owner = adapterReadJson(join(state.buildLeasePath, "owner.json"));
  if (owner?.pid === process.pid) rmSync(state.buildLeasePath, { recursive: true, force: true });
  state.buildLeasePath = null;
  state.buildLeaseOwned = false;
  state.buildLeaseBorrowed = false;
}

function adapterRemoveExactWorktree(state) {
  const path = state.worktreePath;
  if (!path) return;
  const name = relative(state.worktreeRoot, path);
  if (!/^mutation-[A-Za-z0-9._-]+$/.test(name)) throw new Error(`mutation worktree path is invalid: ${path}`);
  const registered = () => {
    const result = spawnSync("git", ["-C", state.root, "worktree", "list", "--porcelain"], { encoding: "utf8" });
    if (result.status !== 0) throw new Error(`mutation worktree list failed: ${String(result.stderr || "").trim()}`);
    return String(result.stdout || "").split("\n").some((line) => line === `worktree ${path}`);
  };
  if (registered()) {
    const result = spawnSync("git", ["-C", state.root, "worktree", "remove", "--force", path], { encoding: "utf8" });
    if (result.status !== 0 && registered()) {
      throw new Error(`mutation worktree removal failed: ${String(result.stderr || "").trim()}`);
    }
  }
  if (existsSync(path)) rmSync(path, { recursive: true, force: true });
  if (registered()) throw new Error(`mutation worktree registration remains: ${path}`);
  state.worktreePath = null;
}

function adapterCleanupSync(state) {
  for (const record of [...state.children]) {
    if (!record.pid) continue;
    try { process.platform === "win32" ? record.child.kill("SIGKILL") : process.kill(-record.pid, "SIGKILL"); } catch { /* exited */ }
  }
  if (state.targetMonitor) clearInterval(state.targetMonitor);
  state.targetMonitor = null;
  const context = state.context;
  if (context?.sourcePath && context.preimage && existsSync(context.sourcePath)) {
    try { adapterWriteDurable(context.sourcePath, context.preimage); } catch { /* exit path preserves worktree removal */ }
  }
  try { adapterRemoveExactWorktree(state); } catch { /* the async path reports cleanup failure */ }
  try { adapterReleaseLock(state); } catch { /* best-effort exit trap */ }
  try { adapterReleaseBuildLease(state); } catch { /* best-effort exit trap */ }
  state.context = null;
}

function adapterRemoveStaleWorktree(state) {
  if (!existsSync(state.worktreePath)) return;
  const marker = adapterReadJson(join(state.worktreePath, ".mutation-owner.json"));
  if (marker?.pid && adapterProcessAlive(marker.pid)) throw new Error("mutation worktree is active");
  const path = state.worktreePath;
  adapterRemoveExactWorktree(state);
  state.worktreePath = path;
}

async function adapterCreateContext(state, mutant, input) {
  if (state.context) throw new Error("mutation adapter already has an active mutant");
  state.mutantId = mutant.id;
  adapterAcquireLock(state);
  const sourceFile = mutant.ast_mutation.source_file.replaceAll("\\", "/");
  const sourcePath = resolve(state.root, sourceFile);
  if (!adapterPathWithin(state.root, sourcePath)) throw new Error(`mutation source escapes checkout: ${sourceFile}`);
  const preimage = readFileSync(sourcePath);
  const preimageSha256 = sha256(preimage);
  state.sourceFile = sourceFile;
  state.compilerSourceSha256 = preimageSha256;
  const context = {
    mutant,
    input,
    sourceFile,
    sourcePath: null,
    preimage,
    preimageSha256,
    sourceBeforeSha256: preimageSha256,
    sourceMutatedSha256: null,
    targetBeforeSha256: adapterFileSha256(join(state.root, "target", "debug", "jet")),
    worktree: state.worktreePath,
    targetDir: join(state.worktreePath, "target"),
    scratch: join(state.worktreePath, ".mutation-scratch"),
    witnessPath: join(state.worktreePath, ".mutation-scratch", `${mutant.id}.jet`),
    baselineObservations: {},
    baselineBuild: null,
    baselineFailure: null,
    targetCapExceeded: false,
    restored: false,
  };
  state.context = context;
  mkdirSync(dirname(state.worktreePath), { recursive: true, mode: 0o700 });
  adapterRemoveStaleWorktree(state);
  adapterGitWorktree(state.root, ["add", "--detach", state.worktreePath, "HEAD"]);
  adapterMirrorBuildInputs(state);
  context.sourcePath = join(state.worktreePath, sourceFile);
  if (!readFileSync(context.sourcePath).equals(preimage)) adapterWriteDurable(context.sourcePath, preimage);
  writeFileSync(join(state.worktreePath, ".mutation-owner.json"), `${JSON.stringify({
    schema: REAL_ADAPTER_SCHEMA,
    pid: process.pid,
    mutant: mutant.id,
    source_file: sourceFile,
    source_sha256: context.preimageSha256,
  })}\n`, { mode: 0o600 });
  mkdirSync(context.scratch, { recursive: true, mode: 0o700 });
  writeFileSync(context.witnessPath, input.witness.source, { mode: 0o600 });

  // Baseline and mutant must use the same freshly built compiler. The shared
  // checkout binary may be stale, so never use it as the baseline oracle.
  context.baselineBuild = await adapterBuildCompiler(state, context);
  if (state.interrupted) throw new Error("mutation interrupted");
  if (context.baselineBuild.ok) {
    for (const tier of adapterTiers(mutant.expected_layer)) {
      if (state.interrupted) throw new Error("mutation interrupted");
      context.baselineObservations[tier] = await adapterRunTier(
        state,
        context,
        context.worktree,
        context.targetDir,
        tier,
      );
    }
    context.baselineFailure = Object.entries(context.baselineObservations)
      .map(([tier, observation]) => ({ tier, failure: adapterObservationFailure(observation) }))
      .find((item) => item.failure) || null;
  }
  if (state.interrupted) throw new Error("mutation interrupted");
  const mutated = applyRustAstMutation({ source: context.preimage, mutant });
  adapterWriteDurable(context.sourcePath, Buffer.from(mutated.source, "utf8"));
  context.sourceMutatedSha256 = mutated.source_sha256;
  return {
    source: mutated.source,
    source_sha256: mutated.source_sha256,
    source_before_sha256: context.preimageSha256,
    source_file: sourceFile,
    worktree: state.worktreePath,
    ast: mutated.ast,
  };
}

function adapterTargetMonitor(state, context) {
  if (state.targetMonitor) clearInterval(state.targetMonitor);
  state.targetMonitor = setInterval(() => {
    const bytes = adapterDiskBytes(context.targetDir);
    if (bytes !== null && bytes > state.targetCapBytes) {
      context.targetCapExceeded = true;
      for (const record of [...state.children]) void adapterTerminate(record);
    }
  }, 1000);
}

function adapterBuildEnvironment(state, context) {
  return adapterEnvironment(state, context, context.worktree, context.targetDir);
}

function adapterBuildCommand(state, context) {
  return {
    program: adapterJetEnv(state, context.worktree),
    args: ["cargo", "build", "-p", "jet", "--bin", "jet", "--jobs", String(REAL_ADAPTER_BUILD_JOBS)],
  };
}

async function adapterBuildCompiler(state, context) {
  if (state.interrupted) return { ok: false, build_failed: true, evidence: "mutation interrupted" };
  adapterTargetMonitor(state, context);
  const command = adapterBuildCommand(state, context);
  let result;
  try {
    result = await adapterRunProcess(state, command.program, command.args, {
      cwd: context.worktree,
      env: adapterBuildEnvironment(state, context),
      timeoutMs: state.timeoutMs,
      label: `mutation:${context.mutant.id}:build`,
    });
  } finally {
    clearInterval(state.targetMonitor);
    state.targetMonitor = null;
  }
  const targetBytes = adapterDiskBytes(context.targetDir);
  const capped = context.targetCapExceeded || targetBytes === null || targetBytes > state.targetCapBytes;
  return {
    ok: result.ok && !capped,
    build_failed: !result.ok || capped,
    command: result.command,
    exit: result.exit,
    signal: result.signal,
    timed_out: result.timed_out,
    evidence: capped ? `mutation target exceeded ${REAL_ADAPTER_TARGET_CAP_GIB}GiB` : adapterResultEvidence(result),
  };
}

function adapterResultEvidence(result) {
  return {
    command: result?.command || null,
    exit: result?.exit ?? null,
    signal: result?.signal || null,
    timeout: result?.timed_out === true,
    stderr: result?.stderr?.toString("utf8").slice(-4000) || "",
  };
}

function adapterOptions(options) {
  const root = resolve(options.root || REAL_ADAPTER_ROOT);
  const worktreeRoot = resolve(root, options.worktree_root || REAL_ADAPTER_WORKTREE_ROOT);
  if (!adapterPathWithin(root, worktreeRoot) && worktreeRoot !== resolve(root, REAL_ADAPTER_WORKTREE_ROOT)) {
    throw new Error("mutation worktree root must be inside .claude/worktrees");
  }
  const timeoutMs = options.timeout_ms === undefined
    ? Number(process.env.JET_HARDENING_MUTATION_TIMEOUT_MS || REAL_ADAPTER_DEFAULT_TIMEOUT_MS)
    : options.timeout_ms;
  if (!Number.isInteger(timeoutMs) || timeoutMs < 1) throw new Error("mutation adapter timeout must be a positive integer");
  const serviceMemoryGib = options.service_memory_gib === undefined
    ? REAL_ADAPTER_SERVICE_MEMORY_GIB
    : options.service_memory_gib;
  if (!Number.isInteger(serviceMemoryGib) || serviceMemoryGib < 1 || serviceMemoryGib > REAL_ADAPTER_SERVICE_MEMORY_GIB) {
    throw new Error(`mutation adapter service memory must be 1 through ${REAL_ADAPTER_SERVICE_MEMORY_GIB}GiB`);
  }
  return {
    root,
    worktreeRoot,
    timeoutMs,
    serviceMemoryGib,
    jetEnv: options.jet_env ? resolve(root, options.jet_env) : null,
  };
}

/** Create the real Layer-4 adapter used by the rig and the single-mutant CLI. */
export function createMutationAdapter(options = {}) {
  const configured = adapterOptions(options);
  const state = {
    ...configured,
    targetCapBytes: REAL_ADAPTER_TARGET_CAP_GIB * REAL_ADAPTER_GIB,
    children: new Set(),
    context: null,
    worktreePath: null,
    targetMonitor: null,
    lockHeld: false,
    buildLeasePath: null,
    buildLeaseOwned: false,
    buildLeaseBorrowed: false,
    compilerSourceSha256: null,
    sourceFile: null,
    mutantId: null,
    interrupted: false,
  };
  const adapter = {
    schema: REAL_ADAPTER_SCHEMA,
    baseline_source: WITNESS_SOURCE,
    current: async () => ({
      source_sha256: state.compilerSourceSha256 || "sha256:unknown-source",
      compiler_source_sha256: state.compilerSourceSha256,
      target_sha256: adapterFileSha256(join(state.root, "target", "debug", "jet")),
      commit: adapterGitHead(state.root),
    }),
    apply: async (mutant, input) => {
      if (adapterDiskBytes(join(state.root, "target")) > state.targetCapBytes) throw new Error("mutation target exceeds 80GiB cap");
      const safeId = mutant.id.replace(/^mutant\./, "").replace(/[^A-Za-z0-9._-]/g, "_");
      state.worktreePath = join(state.worktreeRoot, `mutation-${safeId}`);
      state.mutantId = mutant.id;
      adapterAcquireBuildLease(state);
      try {
        return await adapterCreateContext(state, mutant, input);
      } catch (error) {
        adapterCleanupSync(state);
        throw error;
      }
    },
    build: async (_mutant, _input) => {
      const context = state.context;
      if (!context) return { ok: false, build_failed: true, evidence: "mutation context is missing" };
      if (state.interrupted) return { ok: false, build_failed: true, evidence: "mutation interrupted" };
      if (!state.buildLeasePath) adapterAcquireBuildLease(state);
      if (context.baselineBuild && !context.baselineBuild.ok) {
        return {
          ...context.baselineBuild,
          ok: false,
          build_failed: true,
          baseline_build_failed: true,
        };
      }
      if (context.baselineFailure) {
        return {
          ok: false,
          build_failed: true,
          baseline_proof_failed: true,
          evidence: context.baselineFailure,
        };
      }
      return adapterBuildCompiler(state, context);
    },
    prove: async (layer, _mutant, _input) => {
      const context = state.context;
      if (!context) return adapterProofFailure(layer, null, { failure: "mutation context is missing" });
      if (state.interrupted) return adapterProofFailure(layer, null, { failure: "mutation interrupted" });
      const observations = {};
      for (const tier of adapterTiers(layer)) {
        observations[tier] = await adapterRunTier(state, context, context.worktree, context.targetDir, tier);
        if (state.interrupted) return adapterProofFailure(layer, observations[tier], { failure: "mutation interrupted" });
      }
      return adapterProof(context, layer, observations);
    },
    restore: async (_mutant, _input, _baseline) => {
      const context = state.context;
      if (!context) return { compiler_source_before_sha256: null, compiler_source_after_sha256: null };
      if (context.sourcePath && existsSync(context.sourcePath)) {
        adapterWriteDurable(context.sourcePath, context.preimage);
        const after = readFileSync(context.sourcePath);
        if (!after.equals(context.preimage)) throw new Error(`mutation source restore checksum mismatch: ${context.mutant.id}`);
        context.restored = true;
        const targetAfterSha256 = adapterFileSha256(join(state.root, "target", "debug", "jet"));
        if (targetAfterSha256 !== context.targetBeforeSha256) {
          throw new Error(`mutation target/debug/jet changed outside isolated worktree: ${context.mutant.id}`);
        }
        return {
          compiler_source_before_sha256: context.preimageSha256,
          compiler_source_after_sha256: sha256(after),
          target_debug_jet_before_sha256: context.targetBeforeSha256,
          target_debug_jet_after_sha256: targetAfterSha256,
        };
      }
      throw new Error(`mutation source path disappeared before restore: ${context.mutant.id}`);
    },
    removeWorkspace: async () => {
      if (state.targetMonitor) clearInterval(state.targetMonitor);
      state.targetMonitor = null;
      try {
        adapterRemoveExactWorktree(state);
        state.context = null;
        return { workspace_removed: true };
      } finally {
        adapterReleaseLock(state);
        adapterReleaseBuildLease(state);
      }
    },
    interrupt: async () => {
      state.interrupted = true;
      await Promise.all([...state.children].map((record) => adapterTerminate(record)));
    },
    cleanupSync: () => adapterCleanupSync(state),
    state,
  };
  return Object.freeze(adapter);
}

export function checkMutationAdapterContract(adapter) {
  if (!adapter || adapter.schema !== MUTATION_ADAPTER_CONTRACT.schema) throw new Error("mutation adapter schema is missing");
  for (const method of MUTATION_ADAPTER_CONTRACT.methods) {
    if (typeof adapter[method] !== "function" && !(method === "baseline_source" && typeof adapter[method] === "string")) {
      throw new Error(`mutation adapter method is missing: ${method}`);
    }
  }
  return true;
}

const REAL_MUTATION_ADAPTER = createMutationAdapter();
checkMutationAdapterContract(REAL_MUTATION_ADAPTER);
export const realMutationAdapter = REAL_MUTATION_ADAPTER;
export const baseline_source = REAL_MUTATION_ADAPTER.baseline_source;
export const current = REAL_MUTATION_ADAPTER.current;
export const apply = REAL_MUTATION_ADAPTER.apply;
export const build = REAL_MUTATION_ADAPTER.build;
export const prove = REAL_MUTATION_ADAPTER.prove;
export const restore = REAL_MUTATION_ADAPTER.restore;
export const removeWorkspace = REAL_MUTATION_ADAPTER.removeWorkspace;
export const interrupt = REAL_MUTATION_ADAPTER.interrupt;

function mutationCliOptions(argv) {
  let mutantId = null;
  let timeoutMs;
  let json = false;
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--mutant") mutantId = argv[++index];
    else if (argument === "--timeout-ms") timeoutMs = Number(argv[++index]);
    else if (argument === "--json") json = true;
    else throw new Error(`unknown mutation adapter option: ${argument}`);
  }
  if (!mutantId) throw new Error("usage: hardening-mutation-layer.mjs --mutant <id> [--timeout-ms N] [--json]");
  return { mutantId, timeoutMs, json };
}

function mutationCliOutput(summary, baseline, signal) {
  const row = summary.results?.[0];
  const proof = row?.proof || {};
  const evidence = proof.evidence || {};
  return {
    status: row?.status?.toLowerCase() || summary.status.toLowerCase(),
    mutant_id: row?.mutant_id || null,
    layer: row?.expected_layer || null,
    value_consuming: proof.value_consuming === true,
    observable_mismatch: proof.observable_mismatch === true,
    checksums: {
      compiler_source_before: evidence.compiler_source_before_sha256 || proof.compiler_source_before_sha256 || null,
      compiler_source_mutated: evidence.compiler_source_mutated_sha256 || null,
      compiler_source_after: proof.compiler_source_after_sha256 || null,
      target_debug_jet_before: baseline.target_sha256,
      target_debug_jet_after: proof.current_target_sha256 || null,
    },
    observation: {
      before: evidence.before || null,
      after: evidence.after || null,
      mismatched_tiers: evidence.mismatched_tiers || [],
    },
    baseline_restored: proof.baseline_restored === true,
    workspace_removed: proof.workspace_removed === true,
    build: row?.build || null,
    error: row?.error || null,
    signal,
  };
}

async function mutationCliMain(argv) {
  const options = mutationCliOptions(argv);
  const mutant = MUTATION_CATALOG.find((candidate) => candidate.id === options.mutantId);
  if (!mutant) throw new Error(`unknown mutation catalog id: ${options.mutantId}`);
  const adapter = createMutationAdapter(options.timeoutMs === undefined ? {} : { timeout_ms: options.timeoutMs });
  checkMutationAdapterContract(adapter);
  let requestedSignal = null;
  const signalHandler = (signal) => {
    requestedSignal = signal;
    void adapter.interrupt().catch(() => {});
  };
  process.once("SIGINT", () => signalHandler("SIGINT"));
  process.once("SIGTERM", () => signalHandler("SIGTERM"));
  process.once("exit", () => adapter.cleanupSync());
  const identity = await adapter.current();
  const compilerSource = adapterCheckoutSource(REAL_ADAPTER_ROOT, mutant.ast_mutation.source_file);
  const baseline = {
    source_sha256: sha256(compilerSource),
    target_sha256: identity.target_sha256,
    commit: identity.commit,
    source: mutant.witness.source,
    current: adapter.current,
  };
  let summary;
  try {
    summary = await runMutationSensitivity({
      catalog: [mutant],
      maxMutants: 1,
      baseline,
      apply: adapter.apply,
      build: adapter.build,
      prove: adapter.prove,
      restore: adapter.restore,
      removeWorkspace: adapter.removeWorkspace,
      workspaceRequired: true,
      metadata: { commit: identity.commit, binary_sha256: identity.target_sha256 },
    });
  } finally {
    await adapter.interrupt().catch(() => {});
    adapter.cleanupSync();
  }
  const output = mutationCliOutput(summary, baseline, requestedSignal);
  process.stdout.write(`${options.json ? JSON.stringify(output) : `${output.status} ${output.mutant_id} value_consuming=${output.value_consuming} observable_mismatch=${output.observable_mismatch}\n${JSON.stringify(output.checksums)}\n`}`);
  if (requestedSignal) process.exitCode = 128 + (requestedSignal === "SIGINT" ? 2 : 15);
  else if (output.status !== "killed") process.exitCode = 1;
  return output;
}

if (import.meta.url === `file://${process.argv[1]}` && process.argv.includes("--self-test")) {
  checkMutationCatalogShape();
  console.log("hardening mutation layer: PASS");
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url) && process.argv.includes("--mutant")) {
  mutationCliMain(process.argv.slice(2)).catch((error) => {
    REAL_MUTATION_ADAPTER.cleanupSync();
    process.stderr.write(`hardening mutation adapter: ${error.message}\n`);
    process.exitCode = 1;
  });
}
