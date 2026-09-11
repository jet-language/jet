// D-TEXT-WASM1: text policy stays in the compiled Rust Prelude. This file
// only marshals JavaScript strings through its checked UTF-8 buffer ABI and
// restores the ordinary Web carriers used by MIR dispatch.
const JET_TEXT_WASM_MAX_U32 = 0xffffffff;
const JET_TEXT_WASM_ENCODER = new TextEncoder();
const JET_TEXT_WASM_DECODER = new TextDecoder("utf-8", { fatal: true });

function jet_string_contains(text, needle) {
  return text.includes(needle);
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
