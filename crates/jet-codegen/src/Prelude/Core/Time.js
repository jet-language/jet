// D-TYPE2-TIME1=A: browser time is the same monotonic nanosecond point
// carrier as native Instant; JS only supplies the ambient clock sample.
const JET_TIME_EPOCH_MS = typeof performance === "undefined" ? 0 : performance.now();
function jet_time_monotonic_now_ns() { const now = typeof performance === "undefined" ? 0 : performance.now(); return BigInt(Math.max(0, Math.trunc((now - JET_TIME_EPOCH_MS) * 1000000))); }
function jet_web_duration_ok(value) { return { tag: "Ok", values: [BigInt.asIntN(64, value)] }; }
function jet_web_duration_err(message) { return { tag: "Err", values: [{ message: String(message) }] }; }
function jet_web_duration_from_int(value, scale) { const ns = BigInt(value) * BigInt(scale); return ns < JET_I64_MIN || ns > JET_I64_MAX ? jet_web_duration_err("duration is outside the supported range") : jet_web_duration_ok(ns); }
function jet_web_duration_from_float(value, scale) { const ns = Number(value) * Number(scale); return !Number.isFinite(ns) || ns < Number(JET_I64_MIN) || ns >= 9223372036854775808 ? jet_web_duration_err("duration must be finite and inside the supported range") : jet_web_duration_ok(BigInt(Math.trunc(ns))); }
function jet_duration_scale(value, factor) { const result = BigInt(value) * BigInt(factor); if (result < JET_I64_MIN || result > JET_I64_MAX) jet_runtime_stop("E3010", "", 0, "duration scaling overflowed or divided by zero"); return result; }
function jet_duration_divide(value, factor) { const divisor = BigInt(factor); if (divisor === 0n) jet_runtime_stop("E3010", "", 0, "duration scaling overflowed or divided by zero"); const result = BigInt(value) / divisor; if (result < JET_I64_MIN || result > JET_I64_MAX) jet_runtime_stop("E3010", "", 0, "duration scaling overflowed or divided by zero"); return result; }

function jet_time_add(left, right) { const value = BigInt(left) + BigInt(right); return value > JET_I64_MAX ? JET_I64_MAX : value < JET_I64_MIN ? JET_I64_MIN : value; }
function jet_time_sub(left, right) { const value = BigInt(left) - BigInt(right); return value > JET_I64_MAX ? JET_I64_MAX : value < JET_I64_MIN ? JET_I64_MIN : value; }
