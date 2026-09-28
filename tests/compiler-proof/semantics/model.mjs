#!/usr/bin/env node

/**
 * Executable source semantics for the semantic-observation contract.
 *
 * The input is a deliberately small semantic IR, not a replacement for the
 * production front end.  It gives the checker one honest executable relation
 * for retained witnesses while the generated obligation relation keeps the
 * complete current language/Core/target denominator open until each owner
 * supplies implementation-bound evidence.
 */

export const SEMANTIC_SCHEMA = "jet.semantic-contract.v1";
export const SEMANTIC_SCHEMA_VERSION = 1;
export const MODEL_ID = "jet-semantic-model-v2";
export const OBSERVATION_RELATION = "ordered_effects";
export const MAX_STEPS = 64;
export const MAX_FUEL = 256;
export const MAX_VALUE_DEPTH = 32;
export const MAX_SCHEDULES = 16;

const IDENTIFIER = /^[A-Za-z_][A-Za-z0-9_']*$/u;
const SCALAR_TYPES = new Set(["boolean", "number", "string"]);
const NUMERIC_EXPRESSIONS = new Set([
  "add",
  "sub",
  "subtract",
  "mul",
  "multiply",
  "div",
  "divide",
  "mod",
  "modulo",
]);
const COMPARISON_EXPRESSIONS = new Set([
  "less",
  "less_equal",
  "greater",
  "greater_equal",
]);
const BOOLEAN_EXPRESSIONS = new Set(["and", "or"]);
const COLLECTION_EXPRESSIONS = new Set(["list", "tuple", "map"]);
export const EXPRESSION_KINDS = Object.freeze([
  "literal",
  "var",
  "input",
  ...NUMERIC_EXPRESSIONS,
  "equal",
  ...COMPARISON_EXPRESSIONS,
  "not",
  ...BOOLEAN_EXPRESSIONS,
  "concat",
  "coalesce",
  "if",
  ...COLLECTION_EXPRESSIONS,
  "comptime",
]);
const EXPRESSION_KIND_SET = new Set(EXPRESSION_KINDS);
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
const STATEMENT_KIND_SET = new Set(STATEMENT_KINDS);
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

const DEFAULT_PREMISES = Object.freeze({
  numeric: "finite-number",
  target: "abstract-source-observation",
  scheduler: "single-threaded-main",
  stack_limit: MAX_STEPS,
  allocation_limit: MAX_STEPS,
  foreign_contracts: [],
});

export function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((key) => [key, canonical(value[key])]),
    );
  }
  return value;
}

export function canonicalJson(value) {
  return JSON.stringify(canonical(value));
}

function diagnostic(code, message, path) {
  return { code, message, path };
}

function validIdentifier(value) {
  return typeof value === "string" && IDENTIFIER.test(value);
}

function validValue(value, depth = 0) {
  if (depth > MAX_VALUE_DEPTH) return false;
  if (value === null) return true;
  if (SCALAR_TYPES.has(typeof value)) {
    return typeof value !== "number" || Number.isFinite(value);
  }
  if (Array.isArray(value)) return value.every((item) => validValue(item, depth + 1));
  if (typeof value === "object") {
    return Object.values(value).every((item) => validValue(item, depth + 1));
  }
  return false;
}

function validSchedule(schedule, path) {
  if (!Array.isArray(schedule) || schedule.length === 0 || schedule.some((task) => !validIdentifier(task))) {
    return diagnostic("E-PROOF-SCHEDULE", "schedule must contain one or more task identifiers", path);
  }
  return null;
}

function validatePremises(program) {
  if (program.premises === undefined) return null;
  if (!program.premises || typeof program.premises !== "object" || Array.isArray(program.premises)) {
    return diagnostic("E-PROOF-PREMISES", "premises must be an object", "program.premises");
  }
  const premises = program.premises;
  for (const field of ["numeric", "target", "scheduler"]) {
    if (premises[field] !== undefined && (typeof premises[field] !== "string" || premises[field].length === 0)) {
      return diagnostic("E-PROOF-PREMISES", `${field} premise must be named`, `program.premises.${field}`);
    }
  }
  for (const field of ["stack_limit", "allocation_limit"]) {
    if (premises[field] !== undefined
        && (!Number.isInteger(premises[field]) || premises[field] < 1 || premises[field] > MAX_FUEL)) {
      return diagnostic("E-PROOF-PREMISES", `${field} must be an integer in 1..${MAX_FUEL}`, `program.premises.${field}`);
    }
  }
  if (premises.foreign_contracts !== undefined
      && (!Array.isArray(premises.foreign_contracts)
        || premises.foreign_contracts.some((entry) => typeof entry !== "string" || entry.length === 0))) {
    return diagnostic("E-PROOF-PREMISES", "foreign_contracts must contain named contracts", "program.premises.foreign_contracts");
  }
  return null;
}

function validateSchedules(program) {
  if (program.allowed_schedules === undefined) return null;
  if (!Array.isArray(program.allowed_schedules)
      || program.allowed_schedules.length === 0
      || program.allowed_schedules.length > MAX_SCHEDULES) {
    return diagnostic("E-PROOF-SCHEDULE", `allowed_schedules must contain 1..${MAX_SCHEDULES} schedules`, "program.allowed_schedules");
  }
  for (const [index, schedule] of program.allowed_schedules.entries()) {
    const error = validSchedule(schedule, `program.allowed_schedules[${index}]`);
    if (error) return error;
  }
  return null;
}

function staticExpressionError(expression, path, comptime = false) {
  if (!expression || typeof expression !== "object" || Array.isArray(expression)) {
    return diagnostic("E-PROOF-SHAPE", "expression must be an object", path);
  }
  const kind = expression.kind;
  if (!EXPRESSION_KIND_SET.has(kind)) {
    return diagnostic("E-PROOF-EXPR", `unsupported expression kind ${String(kind)}`, `${path}.kind`);
  }
  if (comptime && (kind === "var" || kind === "input")) {
    return diagnostic(
      "E-PROOF-COMPTIME",
      `${kind} is not available in compile-time evaluation`,
      `${path}.kind`,
    );
  }
  switch (kind) {
    case "literal":
      if (!validValue(expression.value)) {
        return diagnostic("E-PROOF-VALUE", "literal must be a finite JSON value", `${path}.value`);
      }
      return null;
    case "var":
    case "input":
      if (!validIdentifier(expression.name)) {
        return diagnostic("E-PROOF-NAME", "variable name is invalid", `${path}.name`);
      }
      return null;
    case "add":
    case "sub":
    case "subtract":
    case "mul":
    case "multiply":
    case "div":
    case "divide":
    case "mod":
    case "modulo":
    case "equal":
    case "less":
    case "less_equal":
    case "greater":
    case "greater_equal":
    case "and":
    case "or": {
      const left = staticExpressionError(expression.left, `${path}.left`, comptime);
      return left ?? staticExpressionError(expression.right, `${path}.right`, comptime);
    }
    case "not":
    case "coalesce": {
      const value = staticExpressionError(expression.value, `${path}.value`, comptime);
      return value ?? staticExpressionError(expression.fallback, `${path}.fallback`, comptime);
    }
    case "concat": {
      const left = staticExpressionError(expression.left, `${path}.left`, comptime);
      return left ?? staticExpressionError(expression.right, `${path}.right`, comptime);
    }
    case "if": {
      const condition = staticExpressionError(expression.condition, `${path}.condition`, comptime);
      if (condition) return condition;
      const thenError = staticExpressionError(expression.then, `${path}.then`, comptime);
      return thenError ?? staticExpressionError(expression.else, `${path}.else`, comptime);
    }
    case "list":
    case "tuple": {
      if (!Array.isArray(expression.items)) return diagnostic("E-PROOF-SHAPE", "collection items must be an array", `${path}.items`);
      for (const [index, item] of expression.items.entries()) {
        const error = staticExpressionError(item, `${path}.items[${index}]`, comptime);
        if (error) return error;
      }
      return null;
    }
    case "map": {
      if (!Array.isArray(expression.entries)) return diagnostic("E-PROOF-SHAPE", "map entries must be an array", `${path}.entries`);
      for (const [index, entry] of expression.entries.entries()) {
        if (!Array.isArray(entry) || entry.length !== 2) return diagnostic("E-PROOF-SHAPE", "map entry must contain key and value", `${path}.entries[${index}]`);
        const pairPath = `${path}.entries[${index}]`;
        const keyError = staticExpressionError(entry[0], `${pairPath}[0]`, comptime);
        if (keyError) return keyError;
        const valueError = staticExpressionError(entry[1], `${pairPath}[1]`, comptime);
        if (valueError) return valueError;
      }
      return null;
    }
    case "comptime":
      return staticExpressionError(expression.expr, `${path}.expr`, true);
    default:
      return diagnostic("E-PROOF-EXPR", `unsupported expression kind ${String(kind)}`, `${path}.kind`);
  }
}

function validateStatements(steps, names, path, topLevel = false) {
  if (!Array.isArray(steps) || (topLevel && (steps.length === 0 || steps.length > MAX_STEPS))) {
    return diagnostic("E-PROOF-STEPS", `steps must contain 1..${MAX_STEPS} statements`, path);
  }
  let terminal = false;
  for (const [index, statement] of steps.entries()) {
    const statementPath = `${path}[${index}]`;
    if (!statement || typeof statement !== "object" || Array.isArray(statement)) {
      return diagnostic("E-PROOF-SHAPE", "statement must be an object", statementPath);
    }
    const kind = statement.kind;
    if (!STATEMENT_KIND_SET.has(kind)) {
      return diagnostic("E-PROOF-STMT", `unsupported statement kind ${String(kind)}`, `${statementPath}.kind`);
    }
    if (terminal) return diagnostic("E-PROOF-TAIL", "no statement may follow return or fail", statementPath);
    switch (kind) {
      case "let": {
        if (!validIdentifier(statement.name)) return diagnostic("E-PROOF-NAME", "binding name is invalid", `${statementPath}.name`);
        if (names.has(statement.name)) return diagnostic("E-PROOF-DUPLICATE", `binding ${statement.name} is declared twice`, `${statementPath}.name`);
        const error = staticExpressionError(statement.expr, `${statementPath}.expr`);
        if (error) return error;
        names.add(statement.name);
        break;
      }
      case "set":
        if (!validIdentifier(statement.name)) return diagnostic("E-PROOF-NAME", "binding name is invalid", `${statementPath}.name`);
        if (!names.has(statement.name)) return diagnostic("E-PROOF-UNBOUND", `binding ${statement.name} is not declared`, `${statementPath}.name`);
        {
          const error = staticExpressionError(statement.expr, `${statementPath}.expr`);
          if (error) return error;
        }
        break;
      case "emit":
      case "defer":
        if (typeof statement.event !== "string" || statement.event.length === 0) {
          return diagnostic("E-PROOF-EVENT", "event must be a non-empty string", `${statementPath}.event`);
        }
        break;
      case "require":
        if (typeof statement.failure !== "string" || statement.failure.length === 0) {
          return diagnostic("E-PROOF-FAILURE", "require failure must be named", `${statementPath}.failure`);
        }
        {
          const error = staticExpressionError(statement.expr, `${statementPath}.expr`);
          if (error) return error;
        }
        break;
      case "fail":
        if (typeof statement.error !== "string" || statement.error.length === 0) {
          return diagnostic("E-PROOF-FAILURE", "failure must be named", `${statementPath}.error`);
        }
        terminal = true;
        break;
      case "return": {
        const error = staticExpressionError(statement.expr, `${statementPath}.expr`);
        if (error) return error;
        terminal = true;
        break;
      }
      case "if": {
        const condition = staticExpressionError(statement.condition, `${statementPath}.condition`);
        if (condition) return condition;
        const thenError = validateStatements(statement.then, new Set(names), `${statementPath}.then`);
        if (thenError) return thenError;
        if (statement.else !== undefined) {
          const elseError = validateStatements(statement.else, new Set(names), `${statementPath}.else`);
          if (elseError) return elseError;
        }
        break;
      }
      case "block": {
        const error = validateStatements(statement.body, new Set(names), `${statementPath}.body`);
        if (error) return error;
        break;
      }
      case "loop": {
        const hasCount = statement.count !== undefined;
        const hasWhile = statement.while !== undefined;
        if (hasCount === hasWhile) return diagnostic("E-PROOF-LOOP", "loop must provide exactly one count or while expression", statementPath);
        const controlError = staticExpressionError(hasCount ? statement.count : statement.while, `${statementPath}.${hasCount ? "count" : "while"}`);
        if (controlError) return controlError;
        const bodyError = validateStatements(statement.body, new Set(names), `${statementPath}.body`);
        if (bodyError) return bodyError;
        break;
      }
      default:
        return diagnostic("E-PROOF-STMT", `unsupported statement kind ${String(kind)}`, `${statementPath}.kind`);
    }
  }
  return null;
}

export function validateProgram(program) {
  if (!program || typeof program !== "object" || Array.isArray(program)) {
    return diagnostic("E-PROOF-SHAPE", "program must be an object", "program");
  }
  if (program.version !== SEMANTIC_SCHEMA_VERSION) {
    return diagnostic("E-PROOF-VERSION", "unsupported semantic program version", "program.version");
  }
  if (program.input !== undefined && (!program.input || typeof program.input !== "object" || Array.isArray(program.input))) {
    return diagnostic("E-PROOF-INPUT", "input must be an object", "program.input");
  }
  const fuel = program.fuel === undefined ? MAX_FUEL : program.fuel;
  if (!Number.isInteger(fuel) || fuel < 1 || fuel > MAX_FUEL) {
    return diagnostic("E-PROOF-FUEL", `fuel must be an integer in 1..${MAX_FUEL}`, "program.fuel");
  }
  const premisesError = validatePremises(program);
  if (premisesError) return premisesError;
  const schedulesError = validateSchedules(program);
  if (schedulesError) return schedulesError;
  return validateStatements(program.steps, new Set(), "program.steps", true);
}

function observationResult(kind, value, error) {
  if (kind === "failure") return { kind, error };
  return { kind: "value", value };
}

function normalizedPremises(program) {
  const supplied = program.premises ?? {};
  return {
    numeric: supplied.numeric ?? DEFAULT_PREMISES.numeric,
    target: supplied.target ?? DEFAULT_PREMISES.target,
    scheduler: supplied.scheduler ?? DEFAULT_PREMISES.scheduler,
    stack_limit: supplied.stack_limit ?? DEFAULT_PREMISES.stack_limit,
    allocation_limit: supplied.allocation_limit ?? DEFAULT_PREMISES.allocation_limit,
    foreign_contracts: [...(supplied.foreign_contracts ?? DEFAULT_PREMISES.foreign_contracts)],
  };
}

function initialState(program) {
  const allowedSchedules = program.allowed_schedules?.map((schedule) => [...schedule]) ?? [["main"]];
  return {
    bindings: Object.create(null),
    mutations: [],
    consumed_input: [],
    ordered_effects: [],
    cleanup: [],
    deferred: [],
    compile_time: [],
    premises: normalizedPremises(program),
    resource: {
      fuel_limit: program.fuel === undefined ? MAX_FUEL : program.fuel,
      fuel_used: 0,
      stack_limit: normalizedPremises(program).stack_limit,
      stack_used: 1,
      allocation_limit: normalizedPremises(program).allocation_limit,
      allocation_used: 0,
      exhausted: false,
    },
    fuel_limit: program.fuel === undefined ? MAX_FUEL : program.fuel,
    fuel_used: 0,
    schedule: [...allowedSchedules[0]],
    allowed_schedules: allowedSchedules,
  };
}

function snapshotState(state) {
  const resource = {
    ...state.resource,
    exhausted: state.resource.fuel_used >= state.resource.fuel_limit
      || state.resource.allocation_used >= state.resource.allocation_limit
      || state.resource.stack_used > state.resource.stack_limit,
  };
  return {
    mutations: state.mutations.map((entry) => ({ ...entry })),
    consumed_input: [...state.consumed_input],
    ordered_effects: [...state.ordered_effects],
    cleanup: [...state.cleanup],
    compile_time: [...state.compile_time],
    resource,
    resource_premises: {
      numeric: state.premises.numeric,
      target: state.premises.target,
      scheduler: state.premises.scheduler,
      foreign_contracts: [...state.premises.foreign_contracts],
      resource,
    },
    schedule: [...state.schedule],
    allowed_schedules: state.allowed_schedules.map((schedule) => [...schedule]),
  };
}

function runCleanup(state) {
  while (state.deferred.length > 0) {
    const event = state.deferred.pop();
    state.cleanup.push(event);
    state.ordered_effects.push(`cleanup:${event}`);
  }
}

function runCleanupTo(state, depth) {
  while (state.deferred.length > depth) {
    const event = state.deferred.pop();
    state.cleanup.push(event);
    state.ordered_effects.push(`cleanup:${event}`);
  }
}

function consumeFuel(state) {
  if (state.fuel_used >= state.fuel_limit) return false;
  state.fuel_used += 1;
  state.resource.fuel_used = state.fuel_used;
  return true;
}

function consumeAllocation(state) {
  if (state.resource.allocation_used >= state.resource.allocation_limit) return false;
  state.resource.allocation_used += 1;
  return true;
}

function failureValue(error, state) {
  runCleanup(state);
  return {
    result: observationResult("failure", undefined, error),
    state: snapshotState(state),
  };
}

function evaluateExpression(expression, state, input, mode = "runtime") {
  if (!consumeFuel(state)) return { failure: "ResourceExhausted" };
  switch (expression.kind) {
    case "literal":
      return { value: expression.value };
    case "var":
      if (!Object.prototype.hasOwnProperty.call(state.bindings, expression.name)) return { failure: "Unbound" };
      return { value: state.bindings[expression.name] };
    case "input":
      if (mode === "comptime") return { failure: "CompileTimeInput" };
      if (!Object.prototype.hasOwnProperty.call(input, expression.name)) return { failure: "InputUnavailable" };
      state.consumed_input.push(expression.name);
      state.ordered_effects.push(`input:${expression.name}`);
      return { value: input[expression.name] };
    case "add":
    case "sub":
    case "subtract":
    case "mul":
    case "multiply":
    case "div":
    case "divide":
    case "mod":
    case "modulo": {
      const left = evaluateExpression(expression.left, state, input, mode);
      if (left.failure) return left;
      const right = evaluateExpression(expression.right, state, input, mode);
      if (right.failure) return right;
      if (typeof left.value !== "number" || typeof right.value !== "number") return { failure: "TypeError" };
      let value;
      if (expression.kind === "add") value = left.value + right.value;
      else if (expression.kind === "sub" || expression.kind === "subtract") value = left.value - right.value;
      else if (expression.kind === "mul" || expression.kind === "multiply") value = left.value * right.value;
      else if (expression.kind === "div" || expression.kind === "divide") {
        if (right.value === 0) return { failure: "DivisionByZero" };
        value = left.value / right.value;
      } else {
        if (right.value === 0) return { failure: "DivisionByZero" };
        value = left.value % right.value;
      }
      return Number.isFinite(value) ? { value } : { failure: "NumericOverflow" };
    }
    case "equal": {
      const left = evaluateExpression(expression.left, state, input, mode);
      if (left.failure) return left;
      const right = evaluateExpression(expression.right, state, input, mode);
      if (right.failure) return right;
      return { value: canonicalJson(left.value) === canonicalJson(right.value) };
    }
    case "less":
    case "less_equal":
    case "greater":
    case "greater_equal": {
      const left = evaluateExpression(expression.left, state, input, mode);
      if (left.failure) return left;
      const right = evaluateExpression(expression.right, state, input, mode);
      if (right.failure) return right;
      if (typeof left.value !== "number" || typeof right.value !== "number") return { failure: "TypeError" };
      if (expression.kind === "less") return { value: left.value < right.value };
      if (expression.kind === "less_equal") return { value: left.value <= right.value };
      if (expression.kind === "greater") return { value: left.value > right.value };
      return { value: left.value >= right.value };
    }
    case "not": {
      const value = evaluateExpression(expression.value, state, input, mode);
      if (value.failure) return value;
      return typeof value.value === "boolean" ? { value: !value.value } : { failure: "TypeError" };
    }
    case "and": {
      const left = evaluateExpression(expression.left, state, input, mode);
      if (left.failure) return left;
      if (typeof left.value !== "boolean") return { failure: "TypeError" };
      if (!left.value) return { value: false };
      const right = evaluateExpression(expression.right, state, input, mode);
      if (right.failure) return right;
      return typeof right.value === "boolean" ? { value: right.value } : { failure: "TypeError" };
    }
    case "or": {
      const left = evaluateExpression(expression.left, state, input, mode);
      if (left.failure) return left;
      if (typeof left.value !== "boolean") return { failure: "TypeError" };
      if (left.value) return { value: true };
      const right = evaluateExpression(expression.right, state, input, mode);
      if (right.failure) return right;
      return typeof right.value === "boolean" ? { value: right.value } : { failure: "TypeError" };
    }
    case "concat": {
      const left = evaluateExpression(expression.left, state, input, mode);
      if (left.failure) return left;
      const right = evaluateExpression(expression.right, state, input, mode);
      if (right.failure) return right;
      if (typeof left.value !== "string" || typeof right.value !== "string") return { failure: "TypeError" };
      return { value: left.value + right.value };
    }
    case "coalesce": {
      const value = evaluateExpression(expression.value, state, input, mode);
      if (value.failure) return value;
      if (value.value !== null && value.value !== undefined) return value;
      return evaluateExpression(expression.fallback, state, input, mode);
    }
    case "if": {
      const condition = evaluateExpression(expression.condition, state, input, mode);
      if (condition.failure) return condition;
      if (typeof condition.value !== "boolean") return { failure: "TypeError" };
      return evaluateExpression(condition.value ? expression.then : expression.else, state, input, mode);
    }
    case "list":
    case "tuple": {
      const values = [];
      for (const item of expression.items) {
        const result = evaluateExpression(item, state, input, mode);
        if (result.failure) return result;
        values.push(result.value);
      }
      return { value: values };
    }
    case "map": {
      const value = Object.create(null);
      for (const entry of expression.entries) {
        const key = evaluateExpression(entry[0], state, input, mode);
        if (key.failure) return key;
        const item = evaluateExpression(entry[1], state, input, mode);
        if (item.failure) return item;
        if (typeof key.value !== "string") return { failure: "TypeError" };
        value[key.value] = item.value;
      }
      return { value };
    }
    case "comptime": {
      const result = evaluateExpression(expression.expr, state, input, "comptime");
      if (result.failure) return result;
      state.compile_time.push(canonical(result.value));
      state.ordered_effects.push(`comptime:${canonicalJson(result.value)}`);
      return result;
    }
    default:
      return { failure: "UnsupportedExpression" };
  }
}

function scopedBindings(state, action) {
  const before = new Set(Object.keys(state.bindings));
  const depth = state.deferred.length;
  const outcome = action();
  runCleanupTo(state, depth);
  for (const name of Object.keys(state.bindings)) {
    if (!before.has(name)) delete state.bindings[name];
  }
  return outcome;
}

function runStatements(steps, state, input) {
  for (const statement of steps) {
    if (!consumeFuel(state)) return { type: "failure", error: "ResourceExhausted" };
    switch (statement.kind) {
      case "let": {
        if (!consumeAllocation(state)) return { type: "failure", error: "ResourceExhausted" };
        const result = evaluateExpression(statement.expr, state, input);
        if (result.failure) return { type: "failure", error: result.failure };
        state.bindings[statement.name] = result.value;
        break;
      }
      case "set": {
        const result = evaluateExpression(statement.expr, state, input);
        if (result.failure) return { type: "failure", error: result.failure };
        const before = state.bindings[statement.name];
        state.bindings[statement.name] = result.value;
        state.mutations.push({ name: statement.name, before, after: result.value });
        state.ordered_effects.push(`mutation:${statement.name}`);
        break;
      }
      case "emit":
        state.ordered_effects.push(`event:${statement.event}`);
        break;
      case "defer":
        state.deferred.push(statement.event);
        break;
      case "require": {
        const result = evaluateExpression(statement.expr, state, input);
        if (result.failure) return { type: "failure", error: result.failure };
        if (typeof result.value !== "boolean") return { type: "failure", error: "TypeError" };
        if (!result.value) return { type: "failure", error: statement.failure };
        break;
      }
      case "fail":
        return { type: "failure", error: statement.error };
      case "return": {
        const result = evaluateExpression(statement.expr, state, input);
        if (result.failure) return { type: "failure", error: result.failure };
        return { type: "return", value: result.value };
      }
      case "if": {
        const condition = evaluateExpression(statement.condition, state, input);
        if (condition.failure) return { type: "failure", error: condition.failure };
        if (typeof condition.value !== "boolean") return { type: "failure", error: "TypeError" };
        const body = condition.value ? statement.then : (statement.else ?? []);
        const outcome = scopedBindings(state, () => runStatements(body, state, input));
        if (outcome.type !== "continue") return outcome;
        break;
      }
      case "block": {
        const outcome = scopedBindings(state, () => runStatements(statement.body, state, input));
        if (outcome.type !== "continue") return outcome;
        break;
      }
      case "loop": {
        if (statement.count !== undefined) {
          const count = evaluateExpression(statement.count, state, input);
          if (count.failure) return { type: "failure", error: count.failure };
          if (!Number.isInteger(count.value) || count.value < 0) return { type: "failure", error: "TypeError" };
          if (count.value > MAX_STEPS) return { type: "failure", error: "ResourceExhausted" };
          for (let index = 0; index < count.value; index += 1) {
            const outcome = scopedBindings(state, () => runStatements(statement.body, state, input));
            if (outcome.type !== "continue") return outcome;
          }
        } else {
          let iterations = 0;
          while (true) {
            if (iterations >= MAX_STEPS) return { type: "failure", error: "ResourceExhausted" };
            const condition = evaluateExpression(statement.while, state, input);
            if (condition.failure) return { type: "failure", error: condition.failure };
            if (typeof condition.value !== "boolean") return { type: "failure", error: "TypeError" };
            if (!condition.value) break;
            iterations += 1;
            const outcome = scopedBindings(state, () => runStatements(statement.body, state, input));
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

/** Evaluate a program. Rejection is a source-processing result, not a runtime failure. */
export function evaluate(program) {
  const error = validateProgram(program);
  if (error) {
    return {
      accepted: false,
      diagnostic: error,
      result: null,
      state: null,
    };
  }
  const input = program.input ?? {};
  const state = initialState(program);
  const outcome = runStatements(program.steps, state, input);
  if (outcome.type === "failure") return { accepted: true, ...failureValue(outcome.error, state) };
  runCleanup(state);
  return {
    accepted: true,
    result: observationResult("value", outcome.type === "return" ? outcome.value : null),
    state: snapshotState(state),
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
    raw: canonicalJson({
      accepted: true,
      ...observation,
      compile_time: state.compile_time,
      mutations: state.mutations,
    }),
    ...observation,
  };
}

export function evaluateObservation(program) {
  return toObservation(evaluate(program));
}

export const contract = Object.freeze({
  schema: SEMANTIC_SCHEMA,
  schema_version: SEMANTIC_SCHEMA_VERSION,
  model_id: MODEL_ID,
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
    process.stderr.write("usage: model.mjs <program.json>\n");
    process.exitCode = 64;
  } else {
    const fs = await import("node:fs");
    const program = JSON.parse(fs.readFileSync(input, "utf8"));
    process.stdout.write(`${canonicalJson(evaluate(program))}\n`);
  }
}
