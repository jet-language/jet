// MIR Web HTTPRouter carrier and typed route registration.
const JET_HTTP_ROUTE_NAME = /^[A-Za-z_][A-Za-z0-9_]*$/;
const JET_HTTP_ROUTE_METHODS = ["GET", "HEAD", "POST", "PUT", "DELETE", "PATCH", "OPTIONS"];
const JET_HTTP_MAX_BODY_BYTES = 1024 * 1024;
const JET_HTTP_TEXT_ENCODER = new TextEncoder();

function jet_http_router_new() {
  return { kind: "http_router", routes: [] };
}

function jet_http_mux_new() {
  return { kind: "http_mux", routes: [] };
}

function jet_http_mux_validate(mux, method, pattern, handler) {
  if (
    mux == null ||
    typeof mux !== "object" ||
    mux.kind !== "http_mux" ||
    !Array.isArray(mux.routes)
  ) {
    throw new Error("invalid HTTP mux");
  }
  if (!JET_HTTP_ROUTE_METHODS.includes(method)) {
    throw new Error(`invalid HTTP method: ${String(method)}`);
  }
  if (typeof handler !== "function") throw new Error("invalid HTTP route handler");
  let parsed;
  try {
    parsed = jet_http_route_parse_pattern(pattern);
  } catch (error) {
    throw new Error(error instanceof Error ? error.message : String(error));
  }
  if (mux.routes.some((route) => route.method === method && route.shape === parsed.shape)) {
    throw new Error(`duplicate route \`${method} ${pattern}\``);
  }
  return parsed;
}

function jet_http_mux_add_handler(mux, method, pattern, handler) {
  const parsed = jet_http_mux_validate(mux, method, pattern, handler);
  mux.routes.push({
    method,
    pattern,
    shape: parsed.shape,
    parsed,
    handler,
    zero: false,
  });
}

function jet_http_mux_add_zero_handler(mux, method, pattern, handler) {
  const parsed = jet_http_mux_validate(mux, method, pattern, handler);
  mux.routes.push({
    method,
    pattern,
    shape: parsed.shape,
    parsed,
    handler,
    zero: true,
  });
}

function jet_http_router_handler_with_names(handler, names) {
  if (typeof handler !== "function" || !Array.isArray(names)) {
    throw new Error("invalid HTTP route handler metadata");
  }
  return Object.assign(
    (...args) => handler(...args),
    { __jet_http_param_names: Object.freeze(names.slice()) },
  );
}

function jet_http_route_decode_segment(segment) {
  let decoded;
  try {
    decoded = decodeURIComponent(segment);
  } catch (_error) {
    throw new Error("route contains invalid percent encoding");
  }
  if (decoded.includes("/") || decoded === "." || decoded === "..") {
    throw new Error("route contains an encoded slash or traversal segment");
  }
  return decoded;
}
function jet_http_mux_dispatch(mux, request) {
  if (
    mux == null ||
    typeof mux !== "object" ||
    mux.kind !== "http_mux" ||
    !Array.isArray(mux.routes)
  ) {
    throw new Error("invalid HTTP mux");
  }
  if (request == null || typeof request !== "object") {
    throw new Error("invalid HTTP request");
  }
  const method = request.method;
  const path = request.path;
  if (typeof method !== "string" || typeof path !== "string") {
    throw new Error("invalid HTTP request carrier");
  }
  let pathSegments;
  try {
    pathSegments = jet_http_route_parse_path(path);
  } catch (_error) {
    return jet_http_bad_request();
  }
  const candidates = mux.routes
    .map((route, index) => ({ route, index }))
    .filter(({ route }) => jet_http_route_matches(route.parsed, pathSegments));
  const methodCandidates = candidates.filter(({ route }) => route.method === method);
  let selected = methodCandidates;
  if (method === "HEAD" && selected.length === 0) {
    selected = candidates.filter(({ route }) => route.method === "GET");
  }
  if (selected.length === 0) {
    return jet_http_result_ok(
      jet_http_response(
        candidates.length === 0 ? 404 : 405,
        candidates.length === 0 ? "404 not found" : "405 method not allowed",
      ),
    );
  }
  const chosen = selected.reduce((best, candidate) => (
    best === null ||
      jet_http_route_compare(
        candidate.route.parsed,
        candidate.index,
        best.route.parsed,
        best.index,
      ) > 0
      ? candidate
      : best
  ), null);
  const route = chosen.route;
  const routedRequest = Object.assign({}, request, {
    params: jet_http_route_params(route.parsed, pathSegments),
    route_template: route.pattern,
  });
  return route.zero ? route.handler() : route.handler(routedRequest);
}


function jet_http_route_parse_pattern(pattern) {
  if (typeof pattern !== "string" || pattern.length === 0 || !pattern.startsWith("/")) {
    throw new Error("route patterns must start with `/`");
  }
  if (pattern.includes("?") || pattern.includes("#")) {
    throw new Error("route patterns cannot contain query or fragment text");
  }
  const rawSegments = pattern === "/" ? [] : pattern.slice(1).split("/");
  const names = new Set();
  const segments = rawSegments.map((segment, index) => {
    if (segment.length === 0) throw new Error("empty path segments are not allowed");
    if (segment.includes("{") || segment.includes("}")) {
      throw new Error("use `:name` or final `*name`; braces are not route markers");
    }
    if (segment === "*") {
      throw new Error("write a named catch-all such as `*wildcard`");
    }
    if (segment.startsWith(":")) {
      const name = segment.slice(1);
      if (!JET_HTTP_ROUTE_NAME.test(name)) {
        throw new Error("parameter names must match `[A-Za-z_][A-Za-z0-9_]*`");
      }
      if (names.has(name)) throw new Error(`duplicate parameter \`${name}\``);
      names.add(name);
      return { kind: "param", name };
    }
    if (segment.startsWith("*")) {
      const name = segment.slice(1);
      if (index !== rawSegments.length - 1) {
        throw new Error("`*name` catch-all must be final");
      }
      if (!JET_HTTP_ROUTE_NAME.test(name)) {
        throw new Error("catch-all names must match `[A-Za-z_][A-Za-z0-9_]*`");
      }
      if (names.has(name)) throw new Error(`duplicate parameter \`${name}\``);
      names.add(name);
      return { kind: "catchall", name };
    }
    return { kind: "static", value: jet_http_route_decode_segment(segment) };
  });
  const shape = segments
    .map((segment) => {
      if (segment.kind === "static") return `s${segment.value.length}:${segment.value}`;
      return segment.kind === "param" ? "p" : "w";
    })
    .join("/");
  return { segments, shape };
}

function jet_http_route_parse_path(path) {
  if (typeof path !== "string" || !path.startsWith("/")) {
    throw new Error("request path must start with `/`");
  }
  const target = path.split("?", 1)[0];
  if (target === "/") return [];
  const rawSegments = target.slice(1).split("/");
  return rawSegments.map((segment) => {
    if (segment.length === 0) throw new Error("request path has an empty segment");
    return jet_http_route_decode_segment(segment);
  });
}

function jet_http_route_matches(parsed, pathSegments) {
  const catchAll = parsed.segments.at(-1)?.kind === "catchall";
  const required = parsed.segments.length - (catchAll ? 1 : 0);
  if (pathSegments.length < required || (!catchAll && pathSegments.length !== required)) {
    return false;
  }
  return parsed.segments.every((segment, index) => {
    if (segment.kind === "static") return pathSegments[index] === segment.value;
    if (segment.kind === "param") return pathSegments[index] !== undefined;
    return true;
  });
}

function jet_http_route_params(parsed, pathSegments) {
  const params = Object.create(null);
  parsed.segments.forEach((segment, index) => {
    if (segment.kind === "param") params[segment.name] = pathSegments[index];
    if (segment.kind === "catchall") params[segment.name] = pathSegments.slice(index).join("/");
  });
  return params;
}

function jet_http_route_rank(segment) {
  return segment.kind === "static" ? 2 : segment.kind === "param" ? 1 : 0;
}

function jet_http_route_compare(left, leftIndex, right, rightIndex) {
  const count = Math.min(left.segments.length, right.segments.length);
  for (let index = 0; index < count; index += 1) {
    const order = jet_http_route_rank(left.segments[index])
      - jet_http_route_rank(right.segments[index]);
    if (order !== 0) return order;
  }
  if (left.segments.length !== right.segments.length) {
    return right.segments.length - left.segments.length;
  }
  return rightIndex - leftIndex;
}

function jet_http_route_query_param(path, name) {
  const marker = path.indexOf("?");
  if (marker < 0) return undefined;
  for (const pair of path.slice(marker + 1).split("&")) {
    const separator = pair.indexOf("=");
    const rawKey = separator < 0 ? pair : pair.slice(0, separator);
    const rawValue = separator < 0 ? "" : pair.slice(separator + 1);
    const key = jet_http_route_decode_segment(rawKey.replaceAll("+", " "));
    if (key !== name) continue;
    return jet_http_route_decode_segment(rawValue.replaceAll("+", " "));
  }
  return undefined;
}

function jet_http_contract_object(value, message) {
  if (value == null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(message);
  }
  return value;
}

function jet_http_contract_schema(value) {
  return value != null && typeof value === "object" && !Array.isArray(value);
}

function jet_http_route_validate_contract(contract, method, pattern, parsed, names) {
  const required = [
    "method",
    "pattern",
    "path",
    "operation_id",
    "summary",
    "parameters",
    "request_body",
    "responses",
    "security",
    "provenance",
  ];
  if (required.some((field) => !Object.prototype.hasOwnProperty.call(contract, field))) {
    throw new Error("HTTP route OpenAPI contract is incomplete");
  }
  if (
    contract.method !== method ||
    contract.pattern !== pattern ||
    typeof contract.path !== "string" ||
    typeof contract.operation_id !== "string" ||
    (contract.summary !== null && typeof contract.summary !== "string") ||
    typeof contract.provenance !== "string"
  ) {
    throw new Error("HTTP route OpenAPI contract has invalid identity fields");
  }
  if (
    !Array.isArray(contract.parameters) ||
    !Array.isArray(contract.responses) ||
    contract.responses.length === 0 ||
    !Array.isArray(contract.security)
  ) {
    throw new Error("HTTP route OpenAPI contract has invalid collections");
  }
  const body = contract.request_body;
  if (
    body !== null &&
    (
      !jet_http_contract_schema(body) ||
      typeof body.required !== "boolean" ||
      typeof body.content_type !== "string" ||
      body.content_type.length === 0 ||
      !jet_http_contract_schema(body.schema)
    )
  ) {
    throw new Error("HTTP route OpenAPI contract has invalid request body");
  }
  const pathNames = new Map();
  for (const segment of parsed.segments) {
    if (segment.kind !== "static") {
      pathNames.set(segment.name, segment.kind === "catchall");
    }
  }
  const parameters = contract.parameters.map((parameter) => {
    const object = jet_http_contract_object(
      parameter,
      "HTTP route parameter contract must be an object",
    );
    if (
      typeof object.name !== "string" ||
      object.name.length === 0 ||
      !["path", "query"].includes(object.in) ||
      typeof object.required !== "boolean" ||
      typeof object.catch_all !== "boolean" ||
      !jet_http_contract_schema(object.schema)
    ) {
      throw new Error("HTTP route parameter contract is incomplete");
    }
    if (object.in === "path") {
      if (!pathNames.has(object.name) || object.required !== true) {
        throw new Error("HTTP route parameter contract has an invalid path binding");
      }
      if (pathNames.get(object.name) !== object.catch_all) {
        throw new Error("HTTP route parameter contract disagrees with its path binding");
      }
    } else if (object.catch_all) {
      throw new Error("HTTP route query parameter cannot be a catch-all");
    }
    return object;
  });
  const parameterNames = new Set();
  for (const parameter of parameters) {
    if (parameterNames.has(parameter.name)) {
      throw new Error(`HTTP route parameter \`${parameter.name}\` is duplicated`);
    }
    parameterNames.add(parameter.name);
  }
  for (const name of pathNames.keys()) {
    if (!parameters.some((parameter) => parameter.in === "path" && parameter.name === name)) {
      throw new Error("HTTP route contract is missing a typed path binding");
    }
  }
  let unbound = 0;
  for (const name of names) {
    if (!parameterNames.has(name)) unbound += 1;
  }
  if (unbound > 1) {
    throw new Error("HTTP route has more than one unbound handler parameter");
  }
  return parameters;
}

function jet_http_schema_matches(value, schema) {
  if (!jet_http_contract_schema(schema)) return false;
  if (Array.isArray(schema.anyOf)) {
    return schema.anyOf.some((candidate) => jet_http_schema_matches(value, candidate));
  }
  if (Array.isArray(schema.oneOf)) {
    return schema.oneOf.filter((candidate) => jet_http_schema_matches(value, candidate)).length === 1;
  }
  if (schema.type === undefined) return true;
  if (schema.type === "null") return value === null;
  if (schema.type === "boolean") return typeof value === "boolean";
  if (schema.type === "integer") return Number.isSafeInteger(value);
  if (schema.type === "number") return typeof value === "number" && Number.isFinite(value);
  if (schema.type === "string") return typeof value === "string";
  if (schema.type === "array") {
    return Array.isArray(value)
      && jet_http_contract_schema(schema.items)
      && value.every((item) => jet_http_schema_matches(item, schema.items));
  }
  if (schema.type !== "object" || value == null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  if (schema.required !== undefined) {
    if (!Array.isArray(schema.required) || schema.required.some((name) => !Object.prototype.hasOwnProperty.call(value, name))) {
      return false;
    }
  }
  if (!jet_http_contract_schema(schema.properties)) return false;
  const additional = schema.additionalProperties === undefined ? true : schema.additionalProperties;
  if (typeof additional !== "boolean") return false;
  return Object.entries(value).every(([name, item]) => {
    if (!Object.prototype.hasOwnProperty.call(schema.properties, name)) return additional;
    return jet_http_schema_matches(item, schema.properties[name]);
  });
}

function jet_http_decode_route_value(raw, schema) {
  let value;
  switch (schema.type) {
    case "string":
      value = raw;
      break;
    case "integer":
      value = Number(raw);
      if (!Number.isSafeInteger(value)) return { ok: false };
      break;
    case "number":
      value = Number(raw);
      if (!Number.isFinite(value)) return { ok: false };
      break;
    case "boolean":
      if (raw !== "true" && raw !== "false") return { ok: false };
      value = raw === "true";
      break;
    case "null":
      if (raw !== "null") return { ok: false };
      value = null;
      break;
    default:
      try {
        value = JSON.parse(raw);
      } catch (_error) {
        value = raw;
      }
      break;
  }
  return jet_http_schema_matches(value, schema) ? { ok: true, value } : { ok: false };
}

function jet_http_request_bytes(request) {
  if (request.body instanceof Uint8Array) return request.body;
  if (typeof request.body === "string") return JET_HTTP_TEXT_ENCODER.encode(request.body);
  if (request.body != null && request.body.bytes instanceof Uint8Array) return request.body.bytes;
  return new Uint8Array();
}

function jet_http_response(status, body) {
  const bytes = typeof body === "string" ? JET_HTTP_TEXT_ENCODER.encode(body) : new Uint8Array(body);
  const responseBody = { kind: "http_body", bytes, text: new TextDecoder().decode(bytes) };
  return {
    kind: "http_response",
    status,
    version: "HTTP/1.1",
    headers: Object.create(null),
    body: responseBody,
    trailers: Object.create(null),
    protocol: "HTTP/1.1",
    remote_address: "",
    redirect_history: [],
    timings_ms: [],
    reused_connection: false,
    raw_content_encoding: null,
  };
}

function jet_http_result_ok(value) {
  return { tag: "Ok", values: [value] };
}

function jet_http_bad_request() {
  return jet_http_result_ok(jet_http_response(400, "400 bad request"));
}
function jet_http_invalid_request_body() {
  return jet_http_result_ok(jet_http_response(400, "invalid request body"));
}

function jet_http_invalid_route_parameter() {
  return jet_http_result_ok(jet_http_response(400, "invalid route parameter"));
}


function jet_http_parse_request(raw) {
  if (typeof raw !== "string") throw new TypeError("HTTP request must be text");
  let source = raw;
  if (!source.includes("\r\n\r\n")) {
    source = `${source.replaceAll("\n", "\r\n")}\r\n\r\n`;
  }
  const boundary = source.indexOf("\r\n\r\n");
  const headerText = source.slice(0, boundary);
  const bodyText = source.slice(boundary + 4);
  const lines = headerText.split("\r\n");
  const requestLine = lines.shift()?.trim().split(/\s+/) ?? [];
  if (
    requestLine.length !== 3 ||
    !requestLine[0] ||
    !requestLine[1]?.startsWith("/") ||
    !["HTTP/1.0", "HTTP/1.1"].includes(requestLine[2])
  ) {
    return jet_http_parse_request("GET / HTTP/1.1\r\n\r\n");
  }
  const headers = Object.create(null);
  for (const line of lines) {
    const separator = line.indexOf(":");
    if (separator <= 0) return jet_http_parse_request("GET / HTTP/1.1\r\n\r\n");
    const name = line.slice(0, separator).trim().toLowerCase();
    const value = line.slice(separator + 1).trim();
    if (!name) return jet_http_parse_request("GET / HTTP/1.1\r\n\r\n");
    headers[name] = headers[name] === undefined ? value : `${headers[name]}, ${value}`;
  }
  return {
    kind: "http_request",
    method: requestLine[0],
    path: requestLine[1],
    url: requestLine[1],
    version: requestLine[2],
    headers,
    body: JET_HTTP_TEXT_ENCODER.encode(bodyText),
    params: Object.create(null),
    route_template: null,
  };
}

function jet_http_router_register(router, method, pattern, handler, file, line, contract_json) {
  if (
    router == null ||
    typeof router !== "object" ||
    router.kind !== "http_router" ||
    !Array.isArray(router.routes)
  ) {
    throw new Error("invalid HTTP router");
  }
  if (!JET_HTTP_ROUTE_METHODS.includes(method)) {
    throw new Error(`invalid HTTP method: ${String(method)}`);
  }
  if (typeof handler !== "function") throw new Error("invalid HTTP route handler");
  if (typeof contract_json !== "string" || contract_json.length === 0) {
    jet_runtime_stop("E2805", file, line, "HTTP route has no checked OpenAPI contract");
  }
  let parsed;
  try {
    parsed = jet_http_route_parse_pattern(pattern);
  } catch (error) {
    jet_runtime_stop("E2805", file, line, error instanceof Error ? error.message : String(error));
  }
  let contract;
  try {
    contract = JSON.parse(contract_json);
  } catch (_error) {
    jet_runtime_stop("E2805", file, line, "HTTP route OpenAPI contract is invalid JSON");
  }
  let names;
  try {
    contract = jet_http_contract_object(contract, "HTTP route OpenAPI contract must be an object");
    names = handler.__jet_http_param_names;
    if (!Array.isArray(names) || names.some((name) => typeof name !== "string" || name.length === 0)) {
      throw new Error("HTTP route handler metadata is missing");
    }
    jet_http_route_validate_contract(contract, method, pattern, parsed, names);
  } catch (error) {
    jet_runtime_stop(
      "E2805",
      file,
      line,
      error instanceof Error ? error.message : String(error),
    );
  }
  if (router.routes.some((route) => route.method === method && route.shape === parsed.shape)) {
    jet_runtime_stop("E2804", file, line, `duplicate route \`${method} ${pattern}\``);
  }
  router.routes.push({
    method,
    pattern,
    shape: parsed.shape,
    parsed,
    handler,
    handler_param_names: names.slice(),
    file: String(file),
    line: Number(line),
    contract_json,
    contract,
  });
}

function jet_http_router_dispatch(router, request) {
  if (
    router == null ||
    typeof router !== "object" ||
    router.kind !== "http_router" ||
    !Array.isArray(router.routes)
  ) {
    throw new Error("invalid HTTP router");
  }
  if (request == null || typeof request !== "object") {
    throw new Error("invalid HTTP request");
  }
  const method = request.method;
  const path = request.path;
  if (typeof method !== "string" || typeof path !== "string") {
    throw new Error("invalid HTTP request carrier");
  }
  let pathSegments;
  try {
    pathSegments = jet_http_route_parse_path(path);
  } catch (_error) {
    return jet_http_bad_request();
  }
  const candidates = router.routes
    .map((route, index) => ({ route, index }))
    .filter(({ route }) => jet_http_route_matches(route.parsed, pathSegments));
  if (candidates.length === 0) return jet_http_result_ok(jet_http_response(404, "404 not found"));
  const methodCandidates = candidates.filter(({ route }) => route.method === method);
  if (methodCandidates.length === 0) {
    return jet_http_result_ok(jet_http_response(405, "405 method not allowed"));
  }
  const selected = methodCandidates.reduce((best, candidate) => (
    best === null ||
      jet_http_route_compare(
        candidate.route.parsed,
        candidate.index,
        best.route.parsed,
        best.index,
      ) > 0
      ? candidate
      : best
  ), null);
  const route = selected.route;
  const params = jet_http_route_params(route.parsed, pathSegments);
  const bodySpec = route.contract.request_body;
  const body = jet_http_request_bytes(request);
  if (bodySpec !== null) {
    if (body.length === 0 && bodySpec.required) return jet_http_invalid_request_body();
    if (body.length !== 0) {
      if (body.length > JET_HTTP_MAX_BODY_BYTES) return jet_http_invalid_request_body();
      let value;
      try {
        value = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(body));
      } catch (_error) {
        return jet_http_invalid_request_body();
      }
      if (!jet_http_schema_matches(value, bodySpec.schema)) return jet_http_invalid_request_body();
    }
  }
  const routedRequest = Object.assign({}, request, {
    params,
    route_template: route.pattern,
  });
  const parameterByName = new Map(
    route.contract.parameters.map((parameter) => [parameter.name, parameter]),
  );
  const args = [];
  let requestSeen = false;
  for (const name of route.handler_param_names) {
    const parameter = parameterByName.get(name);
    if (parameter === undefined) {
      if (requestSeen) return jet_http_invalid_route_parameter();
      requestSeen = true;
      args.push(routedRequest);
      continue;
    }
    let raw;
    try {
      raw = parameter.in === "path" ? params[name] : jet_http_route_query_param(path, name);
    } catch (_error) {
      return jet_http_invalid_route_parameter();
    }
    if (raw === undefined) {
      if (parameter.required) return jet_http_invalid_route_parameter();
      args.push(jet_option_none());
      continue;
    }
    let decoded;
    try {
      decoded = jet_http_decode_route_value(raw, parameter.schema);
    } catch (_error) {
      return jet_http_invalid_route_parameter();
    }
    if (!decoded.ok) return jet_http_invalid_route_parameter();
    args.push(parameter.required ? decoded.value : jet_option_some(decoded.value));
  }
  return route.handler(...args);
}
