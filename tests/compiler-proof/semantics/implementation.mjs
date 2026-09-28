#!/usr/bin/env node

/**
 * Independent executable candidate for the bounded semantic-observation
 * relation.  This intentionally does not import model.mjs: agreement is
 * evidence only when both implementations independently produce the same
 * complete observation, and the generated obligation relation remains the
 * complete proof denominator.
 */

export const SEMANTIC_SCHEMA = "jet.semantic-contract.v1";
export const SEMANTIC_SCHEMA_VERSION = 1;
export const IMPLEMENTATION_ID = "jet-semantic-implementation-v2";
export const OBSERVATION_RELATION = "ordered_effects";
export const MAX_STEPS = 64;
export const MAX_FUEL = 256;
export const MAX_VALUE_DEPTH = 32;
export const MAX_SCHEDULES = 16;

const NAME_RE = /^[A-Za-z_][A-Za-z0-9_']*$/u;
const NUMERIC = new Set(["add", "sub", "subtract", "mul", "multiply", "div", "divide", "mod", "modulo"]);
const EXPRESSIONS = Object.freeze([
  "literal",
  "var",
  "input",
  ...NUMERIC,
  "equal",
  "less",
  "less_equal",
  "greater",
  "greater_equal",
  "not",
  "and",
  "or",
  "concat",
  "coalesce",
  "if",
  "list",
  "tuple",
  "map",
  "comptime",
]);
const EXPRESSION_SET = new Set(EXPRESSIONS);
export const EXPRESSION_KINDS = EXPRESSIONS;
export const STATEMENT_KINDS = Object.freeze([
  "let",
  "set",
  "emit",
  "defer",
  "require",
  "fail",
  "return",
  "if",
  "loop",
  "block",
]);
const STATEMENT_SET = new Set(STATEMENT_KINDS);
export const OBSERVATION_FIELDS = Object.freeze([
  "raw",
  "result",
  "typed_failure",
  "mutation",
  "input_consumption",
  "ordered_effects",
  "cleanup",
  "resource_premises",
  "schedule",
  "allowed_schedules",
]);

const DEFAULTS = Object.freeze({
  numeric: "finite-number",
  target: "abstract-source-observation",
  scheduler: "single-threaded-main",
  stack_limit: MAX_STEPS,
  allocation_limit: MAX_STEPS,
  foreign_contracts: [],
});

function ordered(value) {
  if (Array.isArray(value)) return value.map(ordered);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, ordered(value[key])]));
  }
  return value;
}

export function canonical(value) {
  return ordered(value);
}

export function canonicalJson(value) {
  return JSON.stringify(ordered(value));
}

function error(code, message, path) {
  return { code, message, path };
}

function name(value) {
  return typeof value === "string" && NAME_RE.test(value);
}

function jsonValue(value, depth = 0) {
  if (depth > MAX_VALUE_DEPTH || value === undefined) return false;
  if (value === null) return true;
  if (typeof value === "number") return Number.isFinite(value);
  if (typeof value === "boolean" || typeof value === "string") return true;
  if (Array.isArray(value)) return value.every((item) => jsonValue(item, depth + 1));
  if (typeof value === "object") return Object.values(value).every((item) => jsonValue(item, depth + 1));
  return false;
}

function checkSchedule(schedule, path) {
  if (!Array.isArray(schedule) || schedule.length === 0 || schedule.some((entry) => !name(entry))) {
    return error("E-PROOF-SCHEDULE", "schedule must contain one or more task identifiers", path);
  }
  return null;
}

function checkPremises(program) {
  const premises = program.premises;
  if (premises === undefined) return null;
  if (!premises || typeof premises !== "object" || Array.isArray(premises)) {
    return error("E-PROOF-PREMISES", "premises must be an object", "program.premises");
  }
  for (const field of ["numeric", "target", "scheduler"]) {
    if (premises[field] !== undefined && (typeof premises[field] !== "string" || premises[field].length === 0)) {
      return error("E-PROOF-PREMISES", `${field} premise must be named`, `program.premises.${field}`);
    }
  }
  for (const field of ["stack_limit", "allocation_limit"]) {
    if (premises[field] !== undefined && (!Number.isInteger(premises[field]) || premises[field] < 1 || premises[field] > MAX_FUEL)) {
      return error("E-PROOF-PREMISES", `${field} must be an integer in 1..${MAX_FUEL}`, `program.premises.${field}`);
    }
  }
  if (premises.foreign_contracts !== undefined
      && (!Array.isArray(premises.foreign_contracts)
        || premises.foreign_contracts.some((entry) => typeof entry !== "string" || entry.length === 0))) {
    return error("E-PROOF-PREMISES", "foreign_contracts must contain named contracts", "program.premises.foreign_contracts");
  }
  return null;
}

function checkSchedules(program) {
  if (program.allowed_schedules === undefined) return null;
  if (!Array.isArray(program.allowed_schedules)
      || program.allowed_schedules.length === 0
      || program.allowed_schedules.length > MAX_SCHEDULES) {
    return error("E-PROOF-SCHEDULE", `allowed_schedules must contain 1..${MAX_SCHEDULES} schedules`, "program.allowed_schedules");
  }
  for (const [index, schedule] of program.allowed_schedules.entries()) {
    const scheduleError = checkSchedule(schedule, `program.allowed_schedules[${index}]`);
    if (scheduleError) return scheduleError;
  }
  return null;
}

function checkExpression(node, path, compileTime = false) {
  if (!node || typeof node !== "object" || Array.isArray(node)) return error("E-PROOF-SHAPE", "expression must be an object", path);
  if (!EXPRESSION_SET.has(node.kind)) return error("E-PROOF-EXPR", `unsupported expression kind ${String(node.kind)}`, `${path}.kind`);
  if (compileTime && (node.kind === "var" || node.kind === "input")) {
    return error("E-PROOF-COMPTIME", `${node.kind} is not available in compile-time evaluation`, `${path}.kind`);
  }
  if (node.kind === "literal") return jsonValue(node.value) ? null : error("E-PROOF-VALUE", "literal must be a finite JSON value", `${path}.value`);
  if (node.kind === "var" || node.kind === "input") return name(node.name) ? null : error("E-PROOF-NAME", "variable name is invalid", `${path}.name`);
  if (NUMERIC.has(node.kind) || ["equal", "less", "less_equal", "greater", "greater_equal", "and", "or", "concat"].includes(node.kind)) {
    const left = checkExpression(node.left, `${path}.left`, compileTime);
    return left ?? checkExpression(node.right, `${path}.right`, compileTime);
  }
  if (["not", "coalesce"].includes(node.kind)) {
    const valueError = checkExpression(node.value, `${path}.value`, compileTime);
    return valueError ?? checkExpression(node.fallback, `${path}.fallback`, compileTime);
  }
  if (node.kind === "if") {
    const condition = checkExpression(node.condition, `${path}.condition`, compileTime);
    if (condition) return condition;
    const thenError = checkExpression(node.then, `${path}.then`, compileTime);
    return thenError ?? checkExpression(node.else, `${path}.else`, compileTime);
  }
  if (["list", "tuple"].includes(node.kind)) {
    if (!Array.isArray(node.items)) return error("E-PROOF-SHAPE", "collection items must be an array", `${path}.items`);
    for (const [index, item] of node.items.entries()) {
      const itemError = checkExpression(item, `${path}.items[${index}]`, compileTime);
      if (itemError) return itemError;
    }
    return null;
  }
  if (node.kind === "map") {
    if (!Array.isArray(node.entries)) return error("E-PROOF-SHAPE", "map entries must be an array", `${path}.entries`);
    for (const [index, pair] of node.entries.entries()) {
      if (!Array.isArray(pair) || pair.length !== 2) return error("E-PROOF-SHAPE", "map entry must contain key and value", `${path}.entries[${index}]`);
      const pairPath = path + ".entries[" + index + "]";
      const keyError = checkExpression(pair[0], pairPath + "[0]", compileTime);
      if (keyError) return keyError;
      const valueError = checkExpression(pair[1], pairPath + "[1]", compileTime);
      if (valueError) return valueError;
    }
    return null;
  }
  if (node.kind === "comptime") return checkExpression(node.expr, `${path}.expr`, true);
  return error("E-PROOF-EXPR", `unsupported expression kind ${String(node.kind)}`, `${path}.kind`);
}

function checkStatements(statements, bindings, path, top = false) {
  if (!Array.isArray(statements) || (top && (statements.length === 0 || statements.length > MAX_STEPS))) {
    return error("E-PROOF-STEPS", `steps must contain 1..${MAX_STEPS} statements`, path);
  }
  let terminal = false;
  for (const [index, statement] of statements.entries()) {
    const statementPath = `${path}[${index}]`;
    if (!statement || typeof statement !== "object" || Array.isArray(statement)) return error("E-PROOF-SHAPE", "statement must be an object", statementPath);
    if (!STATEMENT_SET.has(statement.kind)) return error("E-PROOF-STMT", `unsupported statement kind ${String(statement.kind)}`, `${statementPath}.kind`);
    if (terminal) return error("E-PROOF-TAIL", "no statement may follow return or fail", statementPath);
    switch (statement.kind) {
      case "let": {
        if (!name(statement.name)) return error("E-PROOF-NAME", "binding name is invalid", `${statementPath}.name`);
        if (bindings.has(statement.name)) return error("E-PROOF-DUPLICATE", `binding ${statement.name} is declared twice`, `${statementPath}.name`);
        const expressionError = checkExpression(statement.expr, `${statementPath}.expr`);
        if (expressionError) return expressionError;
        bindings.add(statement.name);
        break;
      }
      case "set":
        if (!name(statement.name)) return error("E-PROOF-NAME", "binding name is invalid", `${statementPath}.name`);
        if (!bindings.has(statement.name)) return error("E-PROOF-UNBOUND", `binding ${statement.name} is not declared`, `${statementPath}.name`);
        {
          const expressionError = checkExpression(statement.expr, `${statementPath}.expr`);
          if (expressionError) return expressionError;
        }
        break;
      case "emit":
      case "defer":
        if (typeof statement.event !== "string" || statement.event.length === 0) return error("E-PROOF-EVENT", "event must be a non-empty string", `${statementPath}.event`);
        break;
      case "require":
        if (typeof statement.failure !== "string" || statement.failure.length === 0) return error("E-PROOF-FAILURE", "require failure must be named", `${statementPath}.failure`);
        {
          const expressionError = checkExpression(statement.expr, `${statementPath}.expr`);
          if (expressionError) return expressionError;
        }
        break;
      case "fail":
        if (typeof statement.error !== "string" || statement.error.length === 0) return error("E-PROOF-FAILURE", "failure must be named", `${statementPath}.error`);
        terminal = true;
        break;
      case "return": {
        const expressionError = checkExpression(statement.expr, `${statementPath}.expr`);
        if (expressionError) return expressionError;
        terminal = true;
        break;
      }
      case "if": {
        const conditionError = checkExpression(statement.condition, `${statementPath}.condition`);
        if (conditionError) return conditionError;
        const thenError = checkStatements(statement.then, new Set(bindings), `${statementPath}.then`);
        if (thenError) return thenError;
        if (statement.else !== undefined) {
          const elseError = checkStatements(statement.else, new Set(bindings), `${statementPath}.else`);
          if (elseError) return elseError;
        }
        break;
      }
      case "block": {
        const bodyError = checkStatements(statement.body, new Set(bindings), `${statementPath}.body`);
        if (bodyError) return bodyError;
        break;
      }
      case "loop": {
        const count = statement.count !== undefined;
        const condition = statement.while !== undefined;
        if (count === condition) return error("E-PROOF-LOOP", "loop must provide exactly one count or while expression", statementPath);
        const expressionError = checkExpression(count ? statement.count : statement.while, `${statementPath}.${count ? "count" : "while"}`);
        if (expressionError) return expressionError;
        const bodyError = checkStatements(statement.body, new Set(bindings), `${statementPath}.body`);
        if (bodyError) return bodyError;
        break;
      }
      default:
        return error("E-PROOF-STMT", `unsupported statement kind ${String(statement.kind)}`, `${statementPath}.kind`);
    }
  }
  return null;
}

export function validateProgram(program) {
  if (!program || typeof program !== "object" || Array.isArray(program)) return error("E-PROOF-SHAPE", "program must be an object", "program");
  if (program.version !== SEMANTIC_SCHEMA_VERSION) return error("E-PROOF-VERSION", "unsupported semantic program version", "program.version");
  if (program.input !== undefined && (!program.input || typeof program.input !== "object" || Array.isArray(program.input))) return error("E-PROOF-INPUT", "input must be an object", "program.input");
  const fuel = program.fuel === undefined ? MAX_FUEL : program.fuel;
  if (!Number.isInteger(fuel) || fuel < 1 || fuel > MAX_FUEL) return error("E-PROOF-FUEL", `fuel must be an integer in 1..${MAX_FUEL}`, "program.fuel");
  const premisesError = checkPremises(program);
  if (premisesError) return premisesError;
  const scheduleError = checkSchedules(program);
  if (scheduleError) return scheduleError;
  return checkStatements(program.steps, new Set(), "program.steps", true);
}

function premises(program) {
  const source = program.premises ?? {};
  return {
    numeric: source.numeric ?? DEFAULTS.numeric,
    target: source.target ?? DEFAULTS.target,
    scheduler: source.scheduler ?? DEFAULTS.scheduler,
    stack_limit: source.stack_limit ?? DEFAULTS.stack_limit,
    allocation_limit: source.allocation_limit ?? DEFAULTS.allocation_limit,
    foreign_contracts: [...(source.foreign_contracts ?? DEFAULTS.foreign_contracts)],
  };
}

function stateFor(program) {
  const allowed = program.allowed_schedules?.map((schedule) => [...schedule]) ?? [["main"]];
  const config = premises(program);
  const fuelLimit = program.fuel === undefined ? MAX_FUEL : program.fuel;
  return {
    vars: Object.create(null),
    edits: [],
    inputs: [],
    effects: [],
    cleaned: [],
    pending: [],
    comptime: [],
    config,
    resources: {
      fuel_limit: fuelLimit,
      fuel_used: 0,
      stack_limit: config.stack_limit,
      stack_used: 1,
      allocation_limit: config.allocation_limit,
      allocation_used: 0,
      exhausted: false,
    },
    fuelLimit,
    fuelUsed: 0,
    schedule: [...allowed[0]],
    allowedSchedules: allowed,
  };
}

function stateSnapshot(state) {
  const resources = {
    ...state.resources,
    exhausted: state.resources.fuel_used >= state.resources.fuel_limit
      || state.resources.allocation_used >= state.resources.allocation_limit
      || state.resources.stack_used > state.resources.stack_limit,
  };
  return {
    mutations: state.edits.map((entry) => ({ ...entry })),
    consumed_input: [...state.inputs],
    ordered_effects: [...state.effects],
    cleanup: [...state.cleaned],
    compile_time: [...state.comptime],
    resource: resources,
    resource_premises: {
      numeric: state.config.numeric,
      target: state.config.target,
      scheduler: state.config.scheduler,
      foreign_contracts: [...state.config.foreign_contracts],
      resource: resources,
    },
    schedule: [...state.schedule],
    allowed_schedules: state.allowedSchedules.map((schedule) => [...schedule]),
  };
}

function fuel(state) {
  if (state.fuelUsed >= state.fuelLimit) return false;
  state.fuelUsed += 1;
  state.resources.fuel_used = state.fuelUsed;
  return true;
}

function allocation(state) {
  if (state.resources.allocation_used >= state.resources.allocation_limit) return false;
  state.resources.allocation_used += 1;
  return true;
}

function clean(state, depth = 0) {
  while (state.pending.length > depth) {
    const event = state.pending.pop();
    state.cleaned.push(event);
    state.effects.push(`cleanup:${event}`);
  }
}

function scope(state, action) {
  const names = new Set(Object.keys(state.vars));
  const depth = state.pending.length;
  const result = action();
  clean(state, depth);
  for (const key of Object.keys(state.vars)) if (!names.has(key)) delete state.vars[key];
  return result;
}

function expr(node, state, input, mode = "runtime") {
  if (!fuel(state)) return { failure: "ResourceExhausted" };
  switch (node.kind) {
    case "literal": return { value: node.value };
    case "var":
      return Object.prototype.hasOwnProperty.call(state.vars, node.name)
        ? { value: state.vars[node.name] }
        : { failure: "Unbound" };
    case "input":
      if (mode === "comptime") return { failure: "CompileTimeInput" };
      if (!Object.prototype.hasOwnProperty.call(input, node.name)) return { failure: "InputUnavailable" };
      state.inputs.push(node.name);
      state.effects.push(`input:${node.name}`);
      return { value: input[node.name] };
    case "add":
    case "sub":
    case "subtract":
    case "mul":
    case "multiply":
    case "div":
    case "divide":
    case "mod":
    case "modulo": {
      const lhs = expr(node.left, state, input, mode);
      if (lhs.failure) return lhs;
      const rhs = expr(node.right, state, input, mode);
      if (rhs.failure) return rhs;
      if (typeof lhs.value !== "number" || typeof rhs.value !== "number") return { failure: "TypeError" };
      let value;
      if (node.kind === "add") value = lhs.value + rhs.value;
      else if (node.kind === "sub" || node.kind === "subtract") value = lhs.value - rhs.value;
      else if (node.kind === "mul" || node.kind === "multiply") value = lhs.value * rhs.value;
      else if (node.kind === "div" || node.kind === "divide") {
        if (rhs.value === 0) return { failure: "DivisionByZero" };
        value = lhs.value / rhs.value;
      } else {
        if (rhs.value === 0) return { failure: "DivisionByZero" };
        value = lhs.value % rhs.value;
      }
      return Number.isFinite(value) ? { value } : { failure: "NumericOverflow" };
    }
    case "equal": {
      const lhs = expr(node.left, state, input, mode);
      if (lhs.failure) return lhs;
      const rhs = expr(node.right, state, input, mode);
      if (rhs.failure) return rhs;
      return { value: canonicalJson(lhs.value) === canonicalJson(rhs.value) };
    }
    case "less":
    case "less_equal":
    case "greater":
    case "greater_equal": {
      const lhs = expr(node.left, state, input, mode);
      if (lhs.failure) return lhs;
      const rhs = expr(node.right, state, input, mode);
      if (rhs.failure) return rhs;
      if (typeof lhs.value !== "number" || typeof rhs.value !== "number") return { failure: "TypeError" };
      if (node.kind === "less") return { value: lhs.value < rhs.value };
      if (node.kind === "less_equal") return { value: lhs.value <= rhs.value };
      if (node.kind === "greater") return { value: lhs.value > rhs.value };
      return { value: lhs.value >= rhs.value };
    }
    case "not": {
      const value = expr(node.value, state, input, mode);
      if (value.failure) return value;
      return typeof value.value === "boolean" ? { value: !value.value } : { failure: "TypeError" };
    }
    case "and": {
      const lhs = expr(node.left, state, input, mode);
      if (lhs.failure) return lhs;
      if (typeof lhs.value !== "boolean") return { failure: "TypeError" };
      if (!lhs.value) return { value: false };
      const rhs = expr(node.right, state, input, mode);
      if (rhs.failure) return rhs;
      return typeof rhs.value === "boolean" ? { value: rhs.value } : { failure: "TypeError" };
    }
    case "or": {
      const lhs = expr(node.left, state, input, mode);
      if (lhs.failure) return lhs;
      if (typeof lhs.value !== "boolean") return { failure: "TypeError" };
      if (lhs.value) return { value: true };
      const rhs = expr(node.right, state, input, mode);
      if (rhs.failure) return rhs;
      return typeof rhs.value === "boolean" ? { value: rhs.value } : { failure: "TypeError" };
    }
    case "concat": {
      const lhs = expr(node.left, state, input, mode);
      if (lhs.failure) return lhs;
      const rhs = expr(node.right, state, input, mode);
      if (rhs.failure) return rhs;
      return typeof lhs.value === "string" && typeof rhs.value === "string"
        ? { value: lhs.value + rhs.value }
        : { failure: "TypeError" };
    }
    case "coalesce": {
      const value = expr(node.value, state, input, mode);
      if (value.failure) return value;
      return value.value === null || value.value === undefined ? expr(node.fallback, state, input, mode) : value;
    }
    case "if": {
      const condition = expr(node.condition, state, input, mode);
      if (condition.failure) return condition;
      if (typeof condition.value !== "boolean") return { failure: "TypeError" };
      return expr(condition.value ? node.then : node.else, state, input, mode);
    }
    case "list":
    case "tuple": {
      const values = [];
      for (const item of node.items) {
        const value = expr(item, state, input, mode);
        if (value.failure) return value;
        values.push(value.value);
      }
      return { value: values };
    }
    case "map": {
      const result = Object.create(null);
      for (const pair of node.entries) {
        const key = expr(pair[0], state, input, mode);
        if (key.failure) return key;
        const value = expr(pair[1], state, input, mode);
        if (value.failure) return value;
        if (typeof key.value !== "string") return { failure: "TypeError" };
        result[key.value] = value.value;
      }
      return { value: result };
    }
    case "comptime": {
      const value = expr(node.expr, state, input, "comptime");
      if (value.failure) return value;
      state.comptime.push(ordered(value.value));
      state.effects.push(`comptime:${canonicalJson(value.value)}`);
      return value;
    }
    default: return { failure: "UnsupportedExpression" };
  }
}

function statements(list, state, input) {
  for (const statement of list) {
    if (!fuel(state)) return { type: "failure", error: "ResourceExhausted" };
    switch (statement.kind) {
      case "let": {
        if (!allocation(state)) return { type: "failure", error: "ResourceExhausted" };
        const value = expr(statement.expr, state, input);
        if (value.failure) return { type: "failure", error: value.failure };
        state.vars[statement.name] = value.value;
        break;
      }
      case "set": {
        const value = expr(statement.expr, state, input);
        if (value.failure) return { type: "failure", error: value.failure };
        const before = state.vars[statement.name];
        state.vars[statement.name] = value.value;
        state.edits.push({ name: statement.name, before, after: value.value });
        state.effects.push(`mutation:${statement.name}`);
        break;
      }
      case "emit":
        state.effects.push(`event:${statement.event}`);
        break;
      case "defer":
        state.pending.push(statement.event);
        break;
      case "require": {
        const condition = expr(statement.expr, state, input);
        if (condition.failure) return { type: "failure", error: condition.failure };
        if (typeof condition.value !== "boolean") return { type: "failure", error: "TypeError" };
        if (!condition.value) return { type: "failure", error: statement.failure };
        break;
      }
      case "fail":
        return { type: "failure", error: statement.error };
      case "return": {
        const value = expr(statement.expr, state, input);
        if (value.failure) return { type: "failure", error: value.failure };
        return { type: "return", value: value.value };
      }
      case "if": {
        const condition = expr(statement.condition, state, input);
        if (condition.failure) return { type: "failure", error: condition.failure };
        if (typeof condition.value !== "boolean") return { type: "failure", error: "TypeError" };
        const body = condition.value ? statement.then : (statement.else ?? []);
        const outcome = scope(state, () => statements(body, state, input));
        if (outcome.type !== "continue") return outcome;
        break;
      }
      case "block": {
        const outcome = scope(state, () => statements(statement.body, state, input));
        if (outcome.type !== "continue") return outcome;
        break;
      }
      case "loop": {
        if (statement.count !== undefined) {
          const count = expr(statement.count, state, input);
          if (count.failure) return { type: "failure", error: count.failure };
          if (!Number.isInteger(count.value) || count.value < 0) return { type: "failure", error: "TypeError" };
          if (count.value > MAX_STEPS) return { type: "failure", error: "ResourceExhausted" };
          for (let index = 0; index < count.value; index += 1) {
            const outcome = scope(state, () => statements(statement.body, state, input));
            if (outcome.type !== "continue") return outcome;
          }
        } else {
          let iterations = 0;
          while (true) {
            if (iterations >= MAX_STEPS) return { type: "failure", error: "ResourceExhausted" };
            const condition = expr(statement.while, state, input);
            if (condition.failure) return { type: "failure", error: condition.failure };
            if (typeof condition.value !== "boolean") return { type: "failure", error: "TypeError" };
            if (!condition.value) break;
            iterations += 1;
            const outcome = scope(state, () => statements(statement.body, state, input));
            if (outcome.type !== "continue") return outcome;
          }
        }
        break;
      }
      default:
        return { type: "failure", error: "UnsupportedStatement" };
    }
  }
  return { type: "continue" };
}

function failure(errorName, state) {
  clean(state);
  return { result: { kind: "failure", error: errorName }, state: stateSnapshot(state) };
}

export function execute(program) {
  const problem = validateProgram(program);
  if (problem) return { accepted: false, diagnostic: problem, result: null, state: null };
  const state = stateFor(program);
  const outcome = statements(program.steps, state, program.input ?? {});
  if (outcome.type === "failure") return { accepted: true, ...failure(outcome.error, state) };
  clean(state);
  return {
    accepted: true,
    result: { kind: "value", value: outcome.type === "return" ? outcome.value : null },
    state: stateSnapshot(state),
  };
}

export function toObservation(evaluation) {
  if (!evaluation.accepted) {
    return {
      raw: canonicalJson({ accepted: false, diagnostic: evaluation.diagnostic }),
      result: null,
      typed_failure: evaluation.diagnostic.code,
      mutation: null,
      input_consumption: [],
      ordered_effects: [],
      cleanup: [],
      resource_premises: null,
      schedule: [],
      allowed_schedules: [],
    };
  }
  const state = evaluation.state;
  const observation = {
    result: evaluation.result,
    typed_failure: evaluation.result.kind === "failure" ? evaluation.result.error : null,
    mutation: state.mutations.length === 0 ? null : canonicalJson(state.mutations),
    input_consumption: [...state.consumed_input],
    ordered_effects: [...state.ordered_effects],
    cleanup: [...state.cleanup],
    resource_premises: state.resource_premises,
    schedule: [...state.schedule],
    allowed_schedules: state.allowed_schedules.map((schedule) => [...schedule]),
  };
  return {
    raw: canonicalJson({ accepted: true, ...observation, compile_time: state.compile_time, mutations: state.mutations }),
    ...observation,
  };
}

export function executeObservation(program) {
  return toObservation(execute(program));
}

export const contract = Object.freeze({
  schema: SEMANTIC_SCHEMA,
  schema_version: SEMANTIC_SCHEMA_VERSION,
  implementation_id: IMPLEMENTATION_ID,
  relation: OBSERVATION_RELATION,
  observations: [
    "result",
    "typed_failure",
    "mutation",
    "input_consumption",
    "ordered_effects",
    "cleanup",
    "resource_premises",
    "schedule",
    "allowed_schedules",
  ],
  supported_expression_kinds: EXPRESSION_KINDS,
  supported_statement_kinds: STATEMENT_KINDS,
  bounds: {
    max_steps: MAX_STEPS,
    max_fuel: MAX_FUEL,
    max_value_depth: MAX_VALUE_DEPTH,
    max_schedules: MAX_SCHEDULES,
  },
});

if (process.argv[1] && import.meta.url === new URL(process.argv[1], "file:").href) {
  const input = process.argv[2];
  if (!input) {
    process.stderr.write("usage: implementation.mjs <program.json>\n");
    process.exitCode = 64;
  } else {
    const fs = await import("node:fs");
    const program = JSON.parse(fs.readFileSync(input, "utf8"));
    process.stdout.write(`${canonicalJson(execute(program))}\n`);
  }
}
