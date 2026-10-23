// I9: Web numeric and unit adapters only marshal values into the compiled
// Prelude. Numeric policy stays in the Rust/Foundation kernels.

const JET_NUMERIC_WEB_U32_MAX = 0xffffffff;
const JET_NUMERIC_WEB_FILE_SLOT = 0;
const JET_NUMERIC_WEB_SCALE_NUM_SLOT = 1;
const JET_NUMERIC_WEB_SCALE_DEN_SLOT = 2;
const JET_NUMERIC_WEB_OFFSET_NUM_SLOT = 3;
const JET_NUMERIC_WEB_OFFSET_DEN_SLOT = 4;
const JET_NUMERIC_WEB_INT_SLOT = 5;

function jet_numeric_web_wasm() {
  const wasm = __jetPreludeWasm;
  const buffer = wasm?.memory?.buffer;
  if (!wasm?.memory || !buffer || typeof buffer.byteLength !== "number") {
    throw new Error("compiled Prelude numeric Web exports are unavailable");
  }
  return wasm;
}

function jet_numeric_web_export(wasm, name) {
  const fn = wasm[name];
  if (typeof fn !== "function") {
    throw new Error(`compiled Prelude numeric Web export ${name} is unavailable`);
  }
  return fn;
}

function jet_numeric_web_number(value, label) {
  const number = Number(value);
  if (!Number.isSafeInteger(number)
      || number < 0
      || number > JET_NUMERIC_WEB_U32_MAX) {
    throw new TypeError(`${label} is not a valid Web integer`);
  }
  return number;
}

function jet_numeric_web_i64(value, label) {
  if (typeof value === "bigint") return BigInt.asIntN(64, value);
  if (typeof value === "number" && Number.isSafeInteger(value)) {
    return BigInt.asIntN(64, BigInt(value));
  }
  throw new TypeError(`${label} must be an exact integer`);
}

function jet_numeric_web_u64(value, label) {
  if (typeof value === "bigint") return BigInt.asUintN(64, value);
  if (typeof value === "number" && Number.isSafeInteger(value)) {
    return BigInt.asUintN(64, BigInt(value));
  }
  throw new TypeError(`${label} must be an exact integer`);
}

function jet_numeric_web_f64(value, label) {
  // Do not reject NaN or infinities here: the compiled conversion kernel owns
  // their Result semantics, and the bits must cross the ABI unchanged.
  if (typeof value !== "number") throw new TypeError(`${label} must be a Float`);
  return value;
}

function jet_numeric_web_flag(value, label) {
  if (value === true || value === 1 || value === 1n) return 1;
  if (value === false || value === 0 || value === 0n) return 0;
  throw new TypeError(`${label} must be a boolean flag`);
}

function jet_numeric_web_bytes(wasm, ptrValue, lenValue, label) {
  const ptr = jet_numeric_web_number(ptrValue, `${label} pointer`);
  const length = jet_numeric_web_number(lenValue, `${label} length`);
  const byteLength = wasm.memory.buffer.byteLength;
  if (ptr > JET_NUMERIC_WEB_U32_MAX || length > JET_NUMERIC_WEB_U32_MAX
      || ptr > byteLength || length > byteLength - ptr) {
    throw new Error(`compiled Prelude numeric Web ${label} buffer is out of bounds`);
  }
  return new Uint8Array(wasm.memory.buffer, ptr, length);
}

function jet_numeric_web_int_text(value) {
  if (typeof value === "bigint") return value.toString();
  if (typeof value === "number" && Number.isSafeInteger(value)) return String(value);
  throw new TypeError("Int must be an exact integer");
}
// MIR uses this one typed boundary for every integer result. Native Web
// adapters may compute with Number internally, but an exact Jet Int leaves
// the boundary as BigInt; fixed-width values are normalized by their MIR
// signedness and width.
function jet_numeric_web_int_value(value, signed = false, bits = 0) {
  let integer;
  if (typeof value === "bigint") {
    integer = value;
  } else if (typeof value === "number" && Number.isSafeInteger(value)) {
    integer = BigInt(value);
  } else {
    throw new TypeError("Jet Int boundary value must be an exact integer");
  }
  if (bits === 0) return integer;
  if (!Number.isInteger(bits) || bits < 1 || bits > 255) {
    throw new RangeError("Jet fixed-width integer has an invalid width");
  }
  return signed ? BigInt.asIntN(bits, integer) : BigInt.asUintN(bits, integer);
}


function jet_numeric_web_text(wasm, slot, value, label) {
  if (typeof value !== "string") throw new TypeError(`${label} must be text`);
  const bytes = new TextEncoder().encode(value);
  if (bytes.length > JET_NUMERIC_WEB_U32_MAX) {
    throw new Error(`numeric Web ${label} exceeds the u32 ABI length`);
  }
  const alloc = jet_numeric_web_export(wasm, "jet_numeric_web_text_alloc");
  const ptr = jet_numeric_web_number(alloc(slot, bytes.length), `${label} allocation`);
  try {
    if (bytes.length !== 0 && ptr === 0) {
      throw new Error(`compiled Prelude numeric Web ${label} allocation failed`);
    }
    if (bytes.length !== 0) {
      jet_numeric_web_bytes(wasm, ptr, bytes.length, label).set(bytes);
    }
    return { slot, ptr, len: bytes.length };
  } catch (error) {
    if (ptr !== 0) jet_numeric_web_release_text(wasm, { slot, ptr }, label);
    throw error;
  }
}

function jet_numeric_web_release_text(wasm, handle, label) {
  const free = jet_numeric_web_export(wasm, "jet_numeric_web_text_free");
  const released = jet_numeric_web_number(free(handle.slot, handle.ptr), `${label} release`);
  if (released !== 1) throw new Error(`compiled Prelude numeric Web ${label} release failed`);
}

function jet_numeric_web_with_text(wasm, entries, body) {
  const handles = [];
  let cleanupError;
  try {
    for (const entry of entries) {
      handles.push(jet_numeric_web_text(wasm, entry.slot, entry.value, entry.label));
    }
    return body(handles);
  } finally {
    for (let index = handles.length - 1; index >= 0; index -= 1) {
      try {
        const handle = handles[index];
        jet_numeric_web_release_text(wasm, handle, entries[index].label);
      } catch (error) {
        cleanupError ??= error;
      }
    }
    if (cleanupError) throw cleanupError;
  }
}

function jet_numeric_web_error(wasm) {
  const length = jet_numeric_web_number(
    jet_numeric_web_export(wasm, "jet_numeric_web_error_len")(),
    "numeric Web error length",
  );
  if (length === 0) return "";
  const ptr = jet_numeric_web_export(wasm, "jet_numeric_web_error_ptr")();
  const bytes = jet_numeric_web_bytes(wasm, ptr, length, "error");
  return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
}

function jet_numeric_web_clear(wasm) {
  jet_numeric_web_export(wasm, "jet_numeric_web_result_clear")();
}

function jet_numeric_web_i128(wasm) {
  const length = jet_numeric_web_number(
    jet_numeric_web_export(wasm, "jet_numeric_web_i128_len")(),
    "numeric Web i128 length",
  );
  if (length !== 16) throw new Error("compiled Prelude numeric Web i128 carrier has an invalid length");
  const ptr = jet_numeric_web_export(wasm, "jet_numeric_web_i128_ptr")();
  const bytes = jet_numeric_web_bytes(wasm, ptr, length, "i128 result");
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let value = view.getBigUint64(0, true) | (view.getBigUint64(8, true) << 64n);
  if ((value & (1n << 127n)) !== 0n) value -= 1n << 128n;
  return value;
}

function jet_numeric_web_result(wasm, statusValue, kind) {
  const status = Number(statusValue);
  try {
    if (status === 1) {
      if (kind === "i128") return { tag: "Ok", values: [jet_numeric_web_i128(wasm)] };
      if (kind === "f64") {
        return { tag: "Ok", values: [jet_numeric_web_export(wasm, "jet_numeric_web_f64_result")()] };
      }
      if (kind === "f32") {
        return { tag: "Ok", values: [jet_numeric_web_export(wasm, "jet_numeric_web_f32_result")()] };
      }
      if (kind === "option_i128") return jet_option_some(jet_numeric_web_i128(wasm));
      throw new Error("unknown numeric Web result carrier");
    }
    if (status === 0) {
      if (kind === "option_i128" || kind === "option_f64") {
        return jet_option_none();
      }
      return { tag: "Err", values: [jet_numeric_web_error(wasm)] };
    }
    throw new Error(jet_numeric_web_error(wasm) || "compiled Prelude numeric Web bridge rejected the call");
  } finally {
    jet_numeric_web_clear(wasm);
  }
}

function jet_numeric_web_checked(wasm, statusValue) {
  const status = Number(statusValue);
  try {
    if (status === 1) return jet_numeric_web_i128(wasm);
    throw new Error(jet_numeric_web_error(wasm) || "compiled Prelude checked numeric Web call failed");
  } finally {
    jet_numeric_web_clear(wasm);
  }
}

function jet_numeric_web_trap(wasm, call, args) {
  try {
    const value = call(...args);
    const error = jet_numeric_web_error(wasm);
    if (error) throw new Error(error);
    return value;
  } finally {
    jet_numeric_web_clear(wasm);
  }
}

function jet_numeric_float_to_int(value, kind) {
  const wasm = jet_numeric_web_wasm();
  const call = jet_numeric_web_export(wasm, "jet_numeric_web_float_to_int");
  return jet_numeric_web_result(
    wasm,
    call(jet_numeric_web_f64(value, "Float"), jet_numeric_web_i64(kind, "destination kind")),
    "i128",
  );
}

function jet_numeric_float_narrow(value) {
  const wasm = jet_numeric_web_wasm();
  const call = jet_numeric_web_export(wasm, "jet_numeric_web_float_narrow");
  return jet_numeric_web_result(wasm, call(jet_numeric_web_f64(value, "Float")), "f32");
}

function jet_numeric_bit_count(value, operation, width) {
  const wasm = jet_numeric_web_wasm();
  const call = jet_numeric_web_export(wasm, "jet_numeric_web_bit_count");
  return call(
    BigInt.asIntN(64, BigInt(value)),
    jet_numeric_web_i64(operation, "population operation"),
    jet_numeric_web_i64(width, "integer width"),
  );
}

function jet_numeric_int_bit_count(value, operation, width) {
  const wasm = jet_numeric_web_wasm();
  const call = jet_numeric_web_export(wasm, "jet_numeric_web_int_bit_count");
  return jet_numeric_web_with_text(
    wasm,
    [{ slot: JET_NUMERIC_WEB_INT_SLOT, value: jet_numeric_web_int_text(value), label: "Int" }],
    ([integer]) => jet_numeric_web_checked(
      wasm,
      call(integer.ptr, integer.len,
        jet_numeric_web_i64(operation, "population operation"),
        jet_numeric_web_i64(width, "integer width")),
    ),
  );
}

function jet_numeric_try_from_fixed(value, signed, kind) {
  const wasm = jet_numeric_web_wasm();
  const call = jet_numeric_web_export(wasm, "jet_numeric_web_try_from_fixed");
  return jet_numeric_web_result(
    wasm,
    call(
      jet_numeric_web_u64(value, "fixed integer"),
      jet_numeric_web_flag(signed, "source signedness"),
      jet_numeric_web_i64(kind, "destination kind"),
    ),
    "i128",
  );
}

function jet_int_try_from(value, kind) {
  const wasm = jet_numeric_web_wasm();
  const call = jet_numeric_web_export(wasm, "jet_numeric_web_int_try_from");
  return jet_numeric_web_with_text(
    wasm,
    [{ slot: JET_NUMERIC_WEB_INT_SLOT, value: jet_numeric_web_int_text(value), label: "Int" }],
    ([intValue]) => jet_numeric_web_result(
      wasm,
      call(intValue.ptr, intValue.len, jet_numeric_web_i64(kind, "destination kind")),
      "option_i128",
    ),
  );
}

function jet_int_try_from_checked(value, kind) {
  const wasm = jet_numeric_web_wasm();
  const call = jet_numeric_web_export(wasm, "jet_numeric_web_int_try_from_checked");
  return jet_numeric_web_with_text(
    wasm,
    [{ slot: JET_NUMERIC_WEB_INT_SLOT, value: jet_numeric_web_int_text(value), label: "Int" }],
    ([intValue]) => jet_numeric_web_result(
      wasm,
      call(intValue.ptr, intValue.len, jet_numeric_web_i64(kind, "destination kind")),
      "i128",
    ),
  );
}

function jet_int_checked_fixed(value, kind, file, line) {
  const wasm = jet_numeric_web_wasm();
  return jet_numeric_web_with_text(
    wasm,
    [
      { slot: JET_NUMERIC_WEB_INT_SLOT, value: jet_numeric_web_int_text(value), label: "Int" },
      { slot: JET_NUMERIC_WEB_FILE_SLOT, value: file, label: "source file" },
    ],
    ([intValue, sourceFile]) => jet_numeric_web_checked(
      wasm,
      jet_numeric_web_export(wasm, "jet_numeric_web_int_checked_fixed")(
        intValue.ptr,
        intValue.len,
        jet_numeric_web_i64(kind, "destination kind"),
        sourceFile.ptr,
        sourceFile.len,
        jet_numeric_web_number(line, "source line"),
      ),
    ),
  );
}

function jet_numeric_checked_widen_at(value, signed, targetF32, file, line) {
  const wasm = jet_numeric_web_wasm();
  return jet_numeric_web_with_text(
    wasm,
    [{ slot: JET_NUMERIC_WEB_FILE_SLOT, value: file, label: "source file" }],
    ([sourceFile]) => jet_numeric_web_trap(
      wasm,
      jet_numeric_web_export(wasm, "jet_numeric_web_checked_widen_at"),
      [
        jet_numeric_web_u64(value, "integer"),
        jet_numeric_web_flag(signed, "source signedness"),
        jet_numeric_web_flag(targetF32, "target width"),
        sourceFile.ptr,
        sourceFile.len,
        jet_numeric_web_number(line, "source line"),
      ],
    ),
  );
}

function jet_int_checked_widen(value, targetF32, file, line) {
  const wasm = jet_numeric_web_wasm();
  return jet_numeric_web_with_text(
    wasm,
    [
      { slot: JET_NUMERIC_WEB_INT_SLOT, value: jet_numeric_web_int_text(value), label: "Int" },
      { slot: JET_NUMERIC_WEB_FILE_SLOT, value: file, label: "source file" },
    ],
    ([intValue, sourceFile]) => jet_numeric_web_trap(
      wasm,
      jet_numeric_web_export(wasm, "jet_numeric_web_int_checked_widen"),
      [
        intValue.ptr,
        intValue.len,
        jet_numeric_web_flag(targetF32, "target width"),
        sourceFile.ptr,
        sourceFile.len,
        jet_numeric_web_number(line, "source line"),
      ],
    ),
  );
}


function jet_numeric_web_unit_texts(scaleNum, scaleDen, offsetNum, offsetDen) {
  return [
    { slot: JET_NUMERIC_WEB_SCALE_NUM_SLOT, value: scaleNum, label: "scale numerator" },
    { slot: JET_NUMERIC_WEB_SCALE_DEN_SLOT, value: scaleDen, label: "scale denominator" },
    { slot: JET_NUMERIC_WEB_OFFSET_NUM_SLOT, value: offsetNum, label: "offset numerator" },
    { slot: JET_NUMERIC_WEB_OFFSET_DEN_SLOT, value: offsetDen, label: "offset denominator" },
  ];
}

function jet_numeric_web_unit_args(handles) {
  return handles.flatMap((handle) => [handle.ptr, handle.len]);
}

function jet_unit_conversion_exact(value, scaleNum, scaleDen, offsetNum, offsetDen) {
  const wasm = jet_numeric_web_wasm();
  const entries = jet_numeric_web_unit_texts(scaleNum, scaleDen, offsetNum, offsetDen);
  return jet_numeric_web_with_text(wasm, entries, (handles) => {
    const args = [jet_numeric_web_f64(value, "Float"), ...jet_numeric_web_unit_args(handles)];
    return jet_numeric_web_result(
      wasm,
      jet_numeric_web_export(wasm, "jet_numeric_web_unit_conversion_exact")(...args),
      "option_f64",
    );
  });
}

function jet_unit_conversion_rounded(value, scaleNum, scaleDen, offsetNum, offsetDen, mode, digits) {
  const wasm = jet_numeric_web_wasm();
  const entries = jet_numeric_web_unit_texts(scaleNum, scaleDen, offsetNum, offsetDen);
  return jet_numeric_web_with_text(wasm, entries, (handles) => {
    const args = [
      jet_numeric_web_f64(value, "Float"),
      ...jet_numeric_web_unit_args(handles),
      jet_numeric_web_i64(mode, "unit rounding mode"),
      jet_numeric_web_i64(digits, "rounding digits"),
    ];
    return jet_numeric_web_result(
      wasm,
      jet_numeric_web_export(wasm, "jet_numeric_web_unit_conversion_rounded")(...args),
      "f64",
    );
  });
}
