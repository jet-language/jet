// D-TYPEDTEXT1: one Web representation of the checked typed-text kernels.
// MIR carries literal and hole order; this module owns only the shared value
// representation and encoding rules.
function jet_typed_sql_raw(template) {
  return { template: String(template), params: [] };
}

function jet_typed_sql_interpolate(literals, holes) {
  const template = [];
  const source = Array.isArray(literals) ? literals : [];
  const values = Array.isArray(holes) ? holes : [];
  for (let index = 0; index < source.length; index += 1) {
    template.push(String(source[index]));
    if (index < values.length) template.push("?");
  }
  return { template: template.join(""), params: values.slice() };
}

function jet_typed_sql_template(value) {
  if (value == null || typeof value !== "object" || !("template" in value)) {
    throw new Error("invalid SQL typed value");
  }
  return String(value.template);
}

function jet_typed_sql_params(value) {
  if (value == null || typeof value !== "object" || !("params" in value)) {
    throw new Error("invalid SQL typed value");
  }
  return Array.isArray(value.params) ? value.params.slice() : [];
}

function jet_typed_html_raw(value) {
  return String(value);
}

function jet_typed_html_text(value) {
  return String(value);
}

function jet_typed_html_escape(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function jet_typed_html_interpolate(literals, holes, trustedHtml) {
  const source = Array.isArray(literals) ? literals : [];
  const values = Array.isArray(holes) ? holes : [];
  const trusted = Array.isArray(trustedHtml) ? trustedHtml : [];
  const output = [];
  for (let index = 0; index < source.length; index += 1) {
    output.push(String(source[index]));
    if (index < values.length) {
      const text = jet_show(values[index]);
      output.push(trusted[index] ? text : jet_typed_html_escape(text));
    }
  }
  return output.join("");
}

function jet_typed_sh_raw(value) {
  const text = String(value).trim();
  return text === "" ? [] : text.split(/\s+/);
}

function jet_typed_sh_interpolate(literals, holes) {
  const source = Array.isArray(literals) ? literals : [];
  const values = Array.isArray(holes) ? holes : [];
  const output = [];
  for (let index = 0; index < source.length; index += 1) {
    const text = String(source[index]).trim();
    if (text !== "") output.push(...text.split(/\s+/));
    if (index < values.length) output.push(jet_show(values[index]));
  }
  return output;
}

function jet_typed_path_component(value) {
  let encoded = encodeURIComponent(String(value));
  encoded = encoded.replaceAll("'", "%27");
  if (encoded === "..") encoded = "%2E%2E";
  return encoded;
}

function jet_typed_path_interpolate(literals, holes) {
  const source = Array.isArray(literals) ? literals : [];
  const values = Array.isArray(holes) ? holes : [];
  const output = [];
  for (let index = 0; index < source.length; index += 1) {
    output.push(String(source[index]));
    if (index < values.length) output.push(jet_typed_path_component(jet_show(values[index])));
  }
  return output.join("");
}

function jet_typed_datetime_interpolate(literals, holes) {
  const source = Array.isArray(literals) ? literals : [];
  const values = Array.isArray(holes) ? holes : [];
  const output = [];
  for (let index = 0; index < source.length; index += 1) {
    output.push(String(source[index]));
    if (index < values.length) output.push(jet_show(values[index]));
  }
  return output.join("");
}

function jet_typed_url_literal(literals, holes) {
  const source = Array.isArray(literals) ? literals : [];
  const values = Array.isArray(holes) ? holes : [];
  const output = [];
  for (let index = 0; index < source.length; index += 1) {
    output.push(String(source[index]));
    if (index < values.length) output.push(jet_show(values[index]));
  }
  return output.join("");
}


function jet_typed_path_literal(literals, holes) {
  return jet_typed_path_interpolate(literals, holes);
}

globalThis["jet_typed_path_literal"] = jet_typed_path_literal;
globalThis["jet_std::jet_typed_url_literal"] = jet_typed_url_literal;
