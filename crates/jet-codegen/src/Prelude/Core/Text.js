// D-TEXT-WASM1: text policy stays in the compiled Rust Prelude. This file
// only marshals JavaScript strings through its checked UTF-8 buffer ABI and
// restores the ordinary Web carriers used by MIR dispatch.
const JET_TEXT_WASM_MAX_U32 = 0xffffffff;
const JET_TEXT_WASM_ENCODER = new TextEncoder();
const JET_TEXT_WASM_DECODER = new TextDecoder("utf-8", { fatal: true });

function jet_string_contains(text, needle) {
  return text.includes(needle);
}
function jet_string_replace(text, from, to) {
  return String(text).replaceAll(String(from), String(to));
}

function jet_cursor_over(text) {
  return { buf: String(text), pos: 0 };
}

function jet_cursor_take_until(cursor, delimiter) {
  const tail = cursor.buf.slice(cursor.pos);
  const needle = String(delimiter);
  const offset = tail.indexOf(needle);
  if (offset < 0) {
    return jet_outcome_err(
      `Cursor.take_until: ${JSON.stringify(needle)} not found in the remaining text`,
    );
  }
  cursor.pos += offset;
  return jet_outcome_ok(tail.slice(0, offset));
}

function jet_cursor_skip_ws(cursor) {
  const tail = cursor.buf.slice(cursor.pos);
  cursor.pos += tail.length - tail.trimStart().length;
}

function jet_text_match_capture(kind, raw) {
  switch (kind.kind) {
    case "text":
      return raw;
    case "int": {
      if (!/^[+-]?[0-9]+$/.test(raw)) return undefined;
      try {
        const value = BigInt(raw);
        const min = -(1n << 63n);
        const max = (1n << 63n) - 1n;
        return value >= min && value <= max ? value : undefined;
      } catch (_) {
        return undefined;
      }
    }
    case "float": {
      if (raw.length === 0 || raw.trim() !== raw) return undefined;
      const value = Number(raw);
      return Number.isNaN(value) ? undefined : value;
    }
    case "bool":
      if (raw === "true" || raw === "True" || raw === "1") return true;
      if (raw === "false" || raw === "False" || raw === "0") return false;
      return undefined;
    case "inline_range": {
      if (!/^[+-]?[0-9]+$/.test(raw)) return undefined;
      try {
        const value = BigInt(raw);
        return value >= BigInt(kind.lo) && value <= BigInt(kind.hi) ? value : undefined;
      } catch (_) {
        return undefined;
      }
    }
    default:
      return undefined;
  }
}

function jet_text_match_scan(subject, parts, consumePrefix) {
  let cursor = 0;
  const captures = [];
  for (let index = 0; index < parts.length; index += 1) {
    const part = parts[index];
    if (part.kind === "literal") {
      if (!subject.slice(cursor).startsWith(part.value)) return undefined;
      cursor += part.value.length;
      continue;
    }
    const next = parts[index + 1];
    const offset = next?.kind === "literal"
      ? subject.slice(cursor).indexOf(next.value)
      : subject.length - cursor;
    if (offset < 0) return undefined;
    const end = cursor + offset;
    const capture = jet_text_match_capture(part.hole_kind, subject.slice(cursor, end));
    if (capture === undefined) return undefined;
    captures.push(capture);
    cursor = end;
  }
  if (!consumePrefix && cursor !== subject.length) return undefined;
  return { consumed: cursor, captures };
}

function jet_cursor_pattern_miss(cursor) {
  return `pattern did not match at cursor position ${cursor.pos}`;
}

function jet_text_pattern_match(subject, parts) {
  const scan = jet_text_match_scan(String(subject), parts, false);
  return scan === undefined ? undefined : scan.captures;
}

function jet_cursor_take_pattern(cursor, parts, fields) {
  const scan = jet_text_match_scan(cursor.buf.slice(cursor.pos), parts, true);
  if (scan === undefined) return jet_outcome_err(jet_cursor_pattern_miss(cursor));
  cursor.pos += scan.consumed;
  const value = {};
  for (let index = 0; index < fields.length; index += 1) {
    value[fields[index]] = scan.captures[index];
  }
  return jet_outcome_ok(fields.length === 0 ? undefined : value);
}
// D-BINPAT1 / I9: mirror the Foundation bit scanner for Web byte-pattern
// matches. The checked MIR route supplies only literal, bit-field, and rest
// descriptors, so captures stay in source order and misses leave no carrier.
function jet_binary_pattern_match(subject, parts) {
  let bytes;
  if (subject instanceof Uint8Array) {
    bytes = subject;
  } else {
    if (!Array.isArray(subject)) {
      throw new TypeError("binary pattern subject is not a byte buffer");
    }
    bytes = new Uint8Array(subject.length);
    for (let index = 0; index < subject.length; index += 1) {
      const value = subject[index];
      if (typeof value === "bigint") {
        if (value < 0n || value > 255n) {
          throw new TypeError("binary pattern subject contains a non-byte value");
        }
        bytes[index] = Number(value);
        continue;
      }
      const integer = Number(value);
      if (!Number.isInteger(integer) || integer < 0 || integer > 255) {
        throw new TypeError("binary pattern subject contains a non-byte value");
      }
      bytes[index] = integer;
    }
  }
  const total = bytes.length * 8;
  let bitPosition = 0;
  const captures = [];
  for (const part of parts) {
    if (part.kind === "literal") {
      if (bitPosition % 8 !== 0) return jet_option_none();
      const start = bitPosition / 8;
      if (start + part.value.length > bytes.length) return jet_option_none();
      for (let index = 0; index < part.value.length; index += 1) {
        if (bytes[start + index] !== part.value[index]) return jet_option_none();
      }
      bitPosition += part.value.length * 8;
      continue;
    }
    if (part.kind === "bits") {
      const width = part.width;
      const end = bitPosition + width;
      if (!Number.isInteger(width) || width < 1 || width > 64 || end > total) {
        return jet_option_none();
      }
      let value = 0n;
      for (let offset = 0; offset < width; offset += 1) {
        const position = bitPosition + offset;
        const byte = bytes[Math.floor(position / 8)];
        const bit = 7 - (position % 8);
        value = (value << 1n) | BigInt((byte >> bit) & 1);
      }
      if (part.little && width % 8 === 0) {
        const byteCount = width / 8;
        let swapped = 0n;
        for (let index = 0; index < byteCount; index += 1) {
          swapped |= ((value >> BigInt(index * 8)) & 0xffn)
            << BigInt((byteCount - index - 1) * 8);
        }
        value = swapped;
      }
      captures.push(value);
      bitPosition = end;
      continue;
    }
    if (part.kind === "rest") {
      if (bitPosition % 8 !== 0) return jet_option_none();
      captures.push(Array.from(bytes.slice(bitPosition / 8), (value) => BigInt(value)));
      bitPosition = total;
      continue;
    }
    throw new TypeError("binary pattern has an unknown part kind");
  }
  if (bitPosition !== total && !parts.some((part) => part.kind === "rest")) {
    return jet_option_none();
  }
  if (bitPosition % 8 !== 0) return jet_option_none();
  return jet_option_some(captures);
}

function jet_text_wasm_exports() {
  let wasm;
  try {
    wasm = __jetPreludeWasm;
  } catch (_error) {
    throw new Error("compiled Prelude text Wasm exports are unavailable");
  }
  if (!wasm || !wasm.memory || !wasm.memory.buffer
      || typeof wasm.jet_text_wasm_input_alloc !== "function"
      || typeof wasm.jet_text_wasm_input_free !== "function"
      || typeof wasm.jet_text_wasm_output_ptr !== "function"
      || typeof wasm.jet_text_wasm_output_len !== "function"
      || typeof wasm.jet_text_wasm_output_clear !== "function") {
    throw new Error("compiled Prelude text Wasm ABI is unavailable");
  }
  return wasm;
}

function jet_text_wasm_operation(wasm, name) {
  const operation = wasm[name];
  if (typeof operation !== "function") {
    throw new Error(`compiled Prelude text Wasm export ${name} is unavailable`);
  }
  return operation;
}

function jet_text_wasm_range(wasm, pointer, length, label) {
  const offset = Number(pointer);
  const size = Number(length);
  if (!Number.isSafeInteger(offset) || offset < 0 || offset > JET_TEXT_WASM_MAX_U32
      || !Number.isSafeInteger(size) || size < 0 || size > JET_TEXT_WASM_MAX_U32) {
    throw new Error(`compiled Prelude text Wasm ${label} range is invalid`);
  }
  if (size !== 0 && offset === 0) {
    throw new Error(`compiled Prelude text Wasm ${label} pointer is null`);
  }
  const capacity = wasm.memory.buffer.byteLength;
  if (offset > capacity || size > capacity - offset) {
    throw new Error(`compiled Prelude text Wasm ${label} range is outside memory`);
  }
  return { offset, size };
}

function jet_text_wasm_encode(value) {
  const bytes = JET_TEXT_WASM_ENCODER.encode(String(value));
  if (bytes.length > JET_TEXT_WASM_MAX_U32) {
    throw new Error("text input is too large for the Wasm bridge");
  }
  return bytes;
}

function jet_text_wasm_with_inputs(wasm, inputs, invoke) {
  const buffers = [];
  wasm.jet_text_wasm_output_clear();
  try {
    for (const input of inputs) {
      const bytes = jet_text_wasm_encode(input.value);
      const pointer = Number(wasm.jet_text_wasm_input_alloc(input.slot, bytes.length));
      if (!Number.isSafeInteger(pointer) || pointer < 0 || pointer > JET_TEXT_WASM_MAX_U32) {
        throw new Error("compiled Prelude text Wasm input allocation is invalid");
      }
      const buffer = { slot: input.slot, pointer, length: bytes.length };
      buffers.push(buffer);
      if (bytes.length !== 0) {
        const range = jet_text_wasm_range(wasm, pointer, bytes.length, "input");
        new Uint8Array(wasm.memory.buffer, range.offset, range.size).set(bytes);
      }
    }
    return invoke(buffers);
  } finally {
    for (let index = buffers.length - 1; index >= 0; index -= 1) {
      const buffer = buffers[index];
      wasm.jet_text_wasm_input_free(buffer.slot, buffer.pointer);
    }
    wasm.jet_text_wasm_output_clear();
  }
}

function jet_text_wasm_read_output(wasm) {
  const length = Number(wasm.jet_text_wasm_output_len());
  if (!Number.isSafeInteger(length) || length < 0 || length > JET_TEXT_WASM_MAX_U32) {
    throw new Error("compiled Prelude text Wasm output length is invalid");
  }
  if (length === 0) return "";
  const pointer = Number(wasm.jet_text_wasm_output_ptr());
  const range = jet_text_wasm_range(wasm, pointer, length, "output");
  const bytes = new Uint8Array(wasm.memory.buffer, range.offset, range.size).slice();
  return JET_TEXT_WASM_DECODER.decode(bytes);
}

function jet_text_wasm_call(name, inputs, invoke) {
  const wasm = jet_text_wasm_exports();
  const operation = jet_text_wasm_operation(wasm, name);
  return jet_text_wasm_with_inputs(wasm, inputs, (buffers) => invoke(wasm, operation, buffers));
}

function jet_text_wasm_bool(value) {
  const result = Number(value);
  if (result !== 0 && result !== 1) {
    throw new Error("compiled Prelude text Wasm returned an invalid Bool");
  }
  return result === 1;
}

function jet_text_pad_start(s, width, fill) {
  return jet_text_wasm_call(
    "jet_text_wasm_pad_start",
    [{ slot: 0, value: s }, { slot: 2, value: fill }],
    (wasm, operation, buffers) => {
      const subject = buffers[0];
      const filler = buffers[1];
      operation(
        subject.pointer,
        subject.length,
        BigInt(width),
        filler.pointer,
        filler.length,
      );
      return jet_text_wasm_read_output(wasm);
    },
  );
}

function jet_text_pad_end(s, width, fill) {
  return jet_text_wasm_call(
    "jet_text_wasm_pad_end",
    [{ slot: 0, value: s }, { slot: 2, value: fill }],
    (wasm, operation, buffers) => {
      const subject = buffers[0];
      const filler = buffers[1];
      operation(
        subject.pointer,
        subject.length,
        BigInt(width),
        filler.pointer,
        filler.length,
      );
      return jet_text_wasm_read_output(wasm);
    },
  );
}

function jet_unicode_index_of(s, needle) {
  return jet_text_wasm_call(
    "jet_text_wasm_index_of",
    [{ slot: 0, value: s }, { slot: 1, value: needle }],
    (_wasm, operation, buffers) => {
      const subject = buffers[0];
      const search = buffers[1];
      const index = BigInt(operation(subject.pointer, subject.length, search.pointer, search.length));
      if (index === -1n) return jet_option_none();
      if (index < 0n) throw new Error("compiled Prelude text Wasm returned an invalid index");
      return jet_option_some(index);
    },
  );
}

function jet_unicode_count(s, needle) {
  return jet_text_wasm_call(
    "jet_text_wasm_count",
    [{ slot: 0, value: s }, { slot: 1, value: needle }],
    (_wasm, operation, buffers) => {
      const subject = buffers[0];
      const search = buffers[1];
      return BigInt(operation(subject.pointer, subject.length, search.pointer, search.length));
    },
  );
}

function jet_text_is_alphabetic(s) {
  return jet_text_wasm_call(
    "jet_text_wasm_is_alphabetic",
    [{ slot: 0, value: s }],
    (_wasm, operation, [subject]) => jet_text_wasm_bool(operation(subject.pointer, subject.length)),
  );
}

function jet_text_is_numeric(s) {
  return jet_text_wasm_call(
    "jet_text_wasm_is_numeric",
    [{ slot: 0, value: s }],
    (_wasm, operation, [subject]) => jet_text_wasm_bool(operation(subject.pointer, subject.length)),
  );
}

function jet_text_is_whitespace(s) {
  return jet_text_wasm_call(
    "jet_text_wasm_is_whitespace",
    [{ slot: 0, value: s }],
    (_wasm, operation, [subject]) => jet_text_wasm_bool(operation(subject.pointer, subject.length)),
  );
}

function jet_text_unicode_is_ascii(s) {
  return jet_text_wasm_call(
    "jet_text_wasm_is_ascii",
    [{ slot: 0, value: s }],
    (_wasm, operation, [subject]) => jet_text_wasm_bool(operation(subject.pointer, subject.length)),
  );
}

function jet_text_title(s) {
  return jet_text_wasm_call(
    "jet_text_wasm_title",
    [{ slot: 0, value: s }],
    (wasm, operation, [subject]) => {
      operation(subject.pointer, subject.length);
      return jet_text_wasm_read_output(wasm);
    },
  );
}
