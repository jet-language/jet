// D-WEBOPENAPI1=A: OpenAPI consumes the checked HTTPRouter route carrier.
// Visual WebRouter routes have no HTTP contract and are not accepted here.
function jet_web_openapi_contract_fail(message) {
  jet_runtime_stop("E2805", "", 0, message);
}

function jet_web_openapi_contract_object(value, message) {
  if (value == null || typeof value !== "object" || Array.isArray(value)) {
    jet_web_openapi_contract_fail(message);
  }
  return value;
}

function jet_web_openapi_contract_text(object, name) {
  if (typeof object[name] !== "string") {
    jet_web_openapi_contract_fail(`HTTP route OpenAPI contract field ${name} must be text`);
  }
  return object[name];
}

function jet_web_openapi_contract_array(object, name) {
  if (!Array.isArray(object[name])) {
    jet_web_openapi_contract_fail(`HTTP route OpenAPI contract field ${name} must be an array`);
  }
  return object[name];
}

function jet_web_openapi_contract_parameter(parameter) {
  const object = jet_web_openapi_contract_object(parameter, "HTTP route parameter contract must be an object");
  const name = jet_web_openapi_contract_text(object, "name");
  const location = jet_web_openapi_contract_text(object, "in");
  if (!Object.prototype.hasOwnProperty.call(object, "required") || typeof object.required !== "boolean") {
    jet_web_openapi_contract_fail("HTTP route parameter contract field required must be boolean");
  }
  if (!Object.prototype.hasOwnProperty.call(object, "schema") || object.schema == null || typeof object.schema !== "object") {
    jet_web_openapi_contract_fail("HTTP route parameter contract field schema must be an object");
  }
  if (!Object.prototype.hasOwnProperty.call(object, "catch_all") || typeof object.catch_all !== "boolean") {
    jet_web_openapi_contract_fail("HTTP route parameter contract field catch_all must be boolean");
  }
  return { name, in: location, required: object.required, schema: object.schema };
}

function jet_web_openapi_contract_response(response) {
  const object = jet_web_openapi_contract_object(response, "HTTP route response contract must be an object");
  if (!Number.isInteger(object.status)) {
    jet_web_openapi_contract_fail("HTTP route response status must be an integer");
  }
  const description = jet_web_openapi_contract_text(object, "description");
  if (!Object.prototype.hasOwnProperty.call(object, "schema")) {
    jet_web_openapi_contract_fail("HTTP route response contract is missing schema");
  }
  if (!Object.prototype.hasOwnProperty.call(object, "content_type")) {
    jet_web_openapi_contract_fail("HTTP route response contract is missing content_type");
  }
  if (object.content_type !== null && typeof object.content_type !== "string") {
    jet_web_openapi_contract_fail("HTTP route response content_type must be text or null");
  }
  const result = { description };
  if (object.content_type !== null) {
    result.content = { [object.content_type]: { schema: object.schema } };
  }
  return [String(object.status), result];
}

function jet_web_openapi_contract_route(route) {
  if (route == null || typeof route !== "object") {
    jet_web_openapi_contract_fail("HTTP route carrier entry must be an object");
  }
  if (typeof route.method !== "string" || typeof route.pattern !== "string" || typeof route.contract_json !== "string") {
    jet_web_openapi_contract_fail("HTTP route carrier entry is incomplete");
  }
  let contract;
  try {
    contract = JSON.parse(route.contract_json);
  } catch (_error) {
    jet_web_openapi_contract_fail("HTTP route OpenAPI contract is invalid JSON");
  }
  contract = jet_web_openapi_contract_object(contract, "HTTP route OpenAPI contract must be an object");
  if (jet_web_openapi_contract_text(contract, "method") !== route.method || jet_web_openapi_contract_text(contract, "pattern") !== route.pattern) {
    jet_web_openapi_contract_fail("HTTP route OpenAPI contract does not match its checked route");
  }
  const path = jet_web_openapi_contract_text(contract, "path");
  const operation_id = jet_web_openapi_contract_text(contract, "operation_id");
  const parameters = jet_web_openapi_contract_array(contract, "parameters").map(jet_web_openapi_contract_parameter);
  const response_entries = jet_web_openapi_contract_array(contract, "responses");
  if (response_entries.length === 0) {
    jet_web_openapi_contract_fail("HTTP route OpenAPI contract has no responses");
  }
  const responses = Object.fromEntries(response_entries.map(jet_web_openapi_contract_response));
  const security_entries = jet_web_openapi_contract_array(contract, "security");
  const security = security_entries.map((entry) => {
    const object = jet_web_openapi_contract_object(entry, "HTTP route security contract must be an object");
    const scheme = jet_web_openapi_contract_text(object, "scheme");
    const scopes = jet_web_openapi_contract_array(object, "scopes");
    if (!scopes.every((scope) => typeof scope === "string")) {
      jet_web_openapi_contract_fail("HTTP route security scopes must be text");
    }
    return { [scheme]: scopes };
  });
  const operation = { operationId: operation_id, parameters, responses };
  if (contract.summary !== null) {
    if (typeof contract.summary !== "string") {
      jet_web_openapi_contract_fail("HTTP route summary must be text or null");
    }
    operation.summary = contract.summary;
  }
  if (!Object.prototype.hasOwnProperty.call(contract, "request_body")) {
    jet_web_openapi_contract_fail("HTTP route OpenAPI contract is missing request_body");
  }
  if (contract.request_body !== null) {
    const body = jet_web_openapi_contract_object(contract.request_body, "HTTP route request body contract must be an object");
    if (typeof body.required !== "boolean" || typeof body.content_type !== "string" || body.schema == null || typeof body.schema !== "object") {
      jet_web_openapi_contract_fail("HTTP route request body contract is incomplete");
    }
    operation.requestBody = { required: body.required, content: { [body.content_type]: { schema: body.schema } } };
  }
  if (security.length > 0) {
    operation.security = security;
  }
  return { path, method: route.method.toLowerCase(), operation };
}

function jet_web_openapi(router) {
  if (router == null || typeof router !== "object" || router.kind !== "http_router" || !Array.isArray(router.routes)) {
    jet_web_openapi_contract_fail("checked HTTPRouter OpenAPI projection is unavailable");
  }
  const entries = router.routes.map(jet_web_openapi_contract_route).sort((left, right) => {
    const path = left.path.localeCompare(right.path);
    return path || left.method.localeCompare(right.method);
  });
  const paths = {};
  for (const entry of entries) {
    if (paths[entry.path] == null) paths[entry.path] = {};
    if (paths[entry.path][entry.method] != null) {
      jet_web_openapi_contract_fail(`duplicate OpenAPI route ${entry.method.toUpperCase()} ${entry.path}`);
    }
    paths[entry.path][entry.method] = entry.operation;
  }
  return JSON.stringify({ openapi: "3.1.0", info: { title: "Jet HTTP Router", version: "1" }, paths });
}
