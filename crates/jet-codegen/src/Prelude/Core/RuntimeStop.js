// D-FAIL-BREACH1=A / I9: the JS adapter's one runtime-stop door.
// The adapter supplies source facts; this function owns report shape and copy.
function jet_list_bounds_message(len, index) {
  return `the list has ${len} items, so position ${index} doesn't exist`;
}

function jet_missing_map_key_message(key) {
  return `the map has no entry for key ${JSON.stringify(String(key))}`;
}

function jet_list_get(base, index, file, line) {
  const position = Number(index);
  if (!Number.isSafeInteger(position) || position < 0 || position >= base.length) {
    jet_runtime_stop("E3010", file, line, jet_list_bounds_message(base.length, index));
  }
  return base[position];
}

function jet_map_get(base, key, file, line) {
  if (!base.has(key)) {
    jet_runtime_stop("E3001", file, line, jet_missing_map_key_message(key));
  }
  return base.get(key);
}

function jet_runtime_stop_report(code, file, line, fn_name, source_line, col, caret_len, message, locals) {
  let what = message;
  let why = "the program reached a registered runtime stop";
  let fix = "handle the condition at the reported source location";
  if (code === "E3001") {
    what = `panic: ${message}`;
    why = "the program reached a panic stop and cannot continue";
    fix = "check the source location and handle the failing condition";
  } else if (code === "E3005") {
    why = "a runtime contract condition evaluated false";
    fix = "satisfy the contract or update it";
  } else if (code === "E3010") {
    why = "the operation has no valid result for these operands";
    fix = "check the operands before the operation, or use a checked operation";
  } else if (code === "E3011") {
    why = "a #Todo hole was reached at runtime";
    fix = "implement this code before running it";
  } else if (code === "E3012") {
    why = "the call stack exceeded Jet's safe runtime limit";
    fix = "end the recursion or make progress toward a base case";
  }

  let rendered = `Stop [${code}]: ${what}\n`;
  if (file) {
    rendered += `  --> ${file}:${line}${fn_name ? ` in ${fn_name}` : ""}\n`;
  }
  if (source_line) {
    const margin = String(line).length;
    const pad = " ".repeat(margin);
    rendered += `   ${pad}|\n`;
    rendered += `${line} | ${source_line}\n`;
    rendered += `   ${pad}| ${" ".repeat(Math.max(0, col - 1))}${"^".repeat(Math.max(1, caret_len))}\n`;
  }
  if (locals) rendered += `locals: ${locals}\n`;
  rendered += ` Why: ${why}\n Fix: ${fix}\n`;
  return rendered;
}

function jet_runtime_stop(code, file, line, message, fn_name = "", source_line = "", locals = "") {
  throw new Error(jet_runtime_stop_report(code, file, line, fn_name, source_line, 1, 1, message, locals));
}
