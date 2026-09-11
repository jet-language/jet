import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const root = dirname(new URL(import.meta.url).pathname);
const repo = resolve(root, "../../../..");
const jetEnv = resolve(repo, "scripts/agent/jet-env");
const inputPath = resolve(root, "input.jet");
const outputPath = resolve(root, "renamed.jet");

function inspect(kind, path) {
    const result = spawnSync(
        jetEnv,
        ["jet", "inspect", "compiler", kind, path],
        { cwd: repo, encoding: "utf8" },
    );
    if (result.error) {
        throw result.error;
    }
    if (result.status !== 0) {
        throw new Error(result.stderr || `jet inspect compiler ${kind} failed`);
    }
    try {
        return JSON.parse(result.stdout);
    } catch (error) {
        throw new Error(`jet inspect compiler ${kind} returned invalid JSON: ${error.message}`);
    }
}

const facts = inspect("lex", inputPath);
const tokens = facts.compiler?.value?.tokens;
if (!Array.isArray(tokens)) {
    throw new Error("compiler lex facts did not contain a token list");
}
const spans = tokens
    .filter((token) => token.kind === "identifier" && token.text === "old_name")
    .map((token) => token.span);
if (spans.length !== 2 || spans.some((span) => !Number.isInteger(span.start) || !Number.isInteger(span.end))) {
    throw new Error(`expected two identifier spans, got ${spans.length}`);
}

const source = readFileSync(inputPath);
const output = Buffer.from(source);
for (let index = spans.length - 1; index >= 0; index -= 1) {
    const span = spans[index];
    output.subarray(span.start, span.end).set(Buffer.from("new_name"));
}
const text = output.toString("utf8");
if ((text.match(/new_name/g) || []).length !== 2) {
    throw new Error("rename did not update exactly two identifiers");
}
if (!text.includes("// keep this comment and old_name text") ||
    !text.includes("// old_name remains in trivia") ||
    !text.includes('print("old_name literal")')) {
    throw new Error("rename changed comment or literal trivia");
}
writeFileSync(outputPath, output);
inspect("parse", outputPath);
inspect("check", outputPath);
console.log("renamed identifiers=2; preserved comment/literal=true; output=renamed.jet");
