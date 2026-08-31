// D-TIMEDEPTH1 / I9: the web tier carries the same civil-time model as the
// Rust Prelude. JavaScript only supplies the ambient clock and stores the
// already-resolved values in exact BigInt fields.
const JET_TIME_EPOCH_MS = typeof performance === "undefined" ? 0 : performance.now();
const JET_TIME_NS_SECOND = 1000000000n;
const JET_TIME_NS_MINUTE = 60000000000n;
const JET_TIME_NS_HOUR = 3600000000000n;
const JET_TIME_NS_DAY = 86400000000000n;
const JET_TIME_NS_WEEK = 604800000000000n;
const JET_TIME_DAY_EPOCH = 719162n;
const JET_TIME_ZONE_DATA = Object.create(null);
const JET_TIME_ZONE_CACHE = Object.create(null);

function jet_time_monotonic_now_ns() {
  const now = typeof performance === "undefined" ? 0 : performance.now();
  return BigInt(Math.max(0, Math.trunc((now - JET_TIME_EPOCH_MS) * 1000000)));
}

function jet_time_floor_div(left, right) {
  let quotient = BigInt(left) / BigInt(right);
  if (BigInt(left) % BigInt(right) < 0n) quotient -= 1n;
  return quotient;
}

function jet_time_mod(left, right) {
  return ((BigInt(left) % BigInt(right)) + BigInt(right)) % BigInt(right);
}

function jet_time_clamp_i64(value) {
  const valueBig = BigInt(value);
  if (valueBig < JET_I64_MIN) return JET_I64_MIN;
  if (valueBig > JET_I64_MAX) return JET_I64_MAX;
  return valueBig;
}
function jet_time_checked_i64(value, unit) {
  const valueBig = BigInt(value);
  if (valueBig < JET_I64_MIN || valueBig > JET_I64_MAX) {
    throw new Error("E2704: Unix epoch " + unit + " do not fit in Int");
  }
  return valueBig;
}


function jet_time_saturating_add_i64(left, right) {
  return jet_time_clamp_i64(BigInt(left) + BigInt(right));
}

function jet_time_saturating_mul_i64(left, right) {
  return jet_time_clamp_i64(BigInt(left) * BigInt(right));
}

function jet_time_saturating_negated_i64(value) {
  const valueBig = BigInt(value);
  return valueBig === JET_I64_MIN ? JET_I64_MAX : -valueBig;
}

function jet_time_saturating_abs_i64(value) {
  const valueBig = BigInt(value);
  return valueBig === JET_I64_MIN ? JET_I64_MAX : valueBig < 0n ? -valueBig : valueBig;
}

function jet_time_pad(value, width, fill) {
  return String(value).padStart(width, fill === undefined ? "0" : fill);
}

function jet_time_year_string(year) {
  const value = BigInt(year);
  const absolute = (value < 0n ? -value : value).toString().padStart(4, "0");
  return (value < 0n ? "-" : "") + absolute;
}

function jet_time_is_leap(year) {
  const value = BigInt(year);
  return value % 4n === 0n && (value % 100n !== 0n || value % 400n === 0n);
}

function jet_time_days_in_month(year, month) {
  switch (Number(month)) {
    case 2: return jet_time_is_leap(year) ? 29n : 28n;
    case 4:
    case 6:
    case 9:
    case 11: return 30n;
    default: return 31n;
  }
}

function jet_time_date(year, month, day) {
  const y = BigInt(year);
  const monthValue = BigInt(month);
  const m = monthValue < 1n ? 1n : monthValue > 12n ? 12n : monthValue;
  const max = jet_time_days_in_month(y, m);
  const dayValue = BigInt(day);
  const d = dayValue < 1n ? 1n : dayValue > max ? max : dayValue;
  return { year: y, month: m, day: d };
}

function jet_time_day_number(date) {
  const year = BigInt(date.year) - 1n;
  const offsets = [0n, 31n, 59n, 90n, 120n, 151n, 181n, 212n, 243n, 273n, 304n, 334n];
  return 365n * year
    + jet_time_floor_div(year, 4n)
    - jet_time_floor_div(year, 100n)
    + jet_time_floor_div(year, 400n)
    + offsets[Number(date.month) - 1]
    + (BigInt(date.month) > 2n && jet_time_is_leap(date.year) ? 1n : 0n)
    + BigInt(date.day) - 1n;
}

function jet_time_date_from_day_number(dayNumber) {
  // Howard Hinnant's 400-year decomposition, shifted to Jet's
  // 0001-01-01 day zero. It does not iterate by year.
  const z = BigInt(dayNumber) - JET_TIME_DAY_EPOCH + 719468n;
  const era = jet_time_floor_div(z, 146097n);
  const dayOfEra = z - era * 146097n;
  const yearOfEra = jet_time_floor_div(
    dayOfEra - jet_time_floor_div(dayOfEra, 1460n)
      + jet_time_floor_div(dayOfEra, 365n)
      - jet_time_floor_div(dayOfEra, 146096n),
    365n,
  );
  let year = yearOfEra + era * 400n;
  const dayOfYear = dayOfEra
    - (365n * yearOfEra + jet_time_floor_div(yearOfEra, 4n) - jet_time_floor_div(yearOfEra, 100n));
  const monthPart = jet_time_floor_div(5n * dayOfYear + 2n, 153n);
  const day = dayOfYear - jet_time_floor_div(153n * monthPart + 2n, 5n) + 1n;
  const month = monthPart + (monthPart < 10n ? 3n : -9n);
  if (month <= 2n) year += 1n;
  return jet_time_date(year, month, day);
}

function jet_time_add_days(date, amount) {
  return jet_time_date_from_day_number(jet_time_day_number(date) + BigInt(amount));
}

function jet_time_add_months(date, amount) {
  const total = BigInt(date.month) - 1n + BigInt(amount);
  const year = BigInt(date.year) + jet_time_floor_div(total, 12n);
  const month = jet_time_mod(total, 12n) + 1n;
  const maximum = jet_time_days_in_month(year, month);
  const day = BigInt(date.day) < maximum ? BigInt(date.day) : maximum;
  return jet_time_date(year, month, day);
}

function jet_time_date_parse(value) {
  const text = String(value);
  const match = /^(-?)([0-9]+)-([0-9]+)-([0-9]+)$/.exec(text);
  if (!match) throw new Error("invalid date: " + text);
  const year = BigInt(match[1] + match[2]);
  const month = BigInt(match[3]);
  const day = BigInt(match[4]);
  if (month < 1n || month > 12n || day < 1n || day > jet_time_days_in_month(year, month)) {
    throw new Error("date out of range: " + text);
  }
  return jet_time_date(year, month, day);
}

function jet_time_weekday(date) {
  return jet_time_mod(jet_time_day_number(date) + 1n, 7n);
}

function jet_time_iso_weekday(date) {
  return jet_time_mod(jet_time_day_number(date), 7n) + 1n;
}

function jet_time_iso_week_year(date) {
  return jet_time_add_days(date, 4n - jet_time_iso_weekday(date)).year;
}

function jet_time_iso_week(date) {
  const thursday = jet_time_add_days(date, 4n - jet_time_iso_weekday(date));
  const januaryFourth = jet_time_date(thursday.year, 1n, 4n);
  const weekOneMonday = jet_time_add_days(januaryFourth, 1n - jet_time_iso_weekday(januaryFourth));
  const thisMonday = jet_time_add_days(thursday, 1n - jet_time_iso_weekday(thursday));
  return jet_time_floor_div(jet_time_day_number(thisMonday) - jet_time_day_number(weekOneMonday), 7n) + 1n;
}

function jet_time_from_iso_week(year, week, weekday) {
  const y = BigInt(year);
  const w = BigInt(week);
  const d = BigInt(weekday);
  if (w < 1n || w > 53n || d < 1n || d > 7n) throw new Error("ISO week date out of range");
  const janFour = jet_time_date(y, 1n, 4n);
  const monday = jet_time_add_days(janFour, 1n - jet_time_iso_weekday(janFour));
  const date = jet_time_add_days(monday, (w - 1n) * 7n + d - 1n);
  if (jet_time_iso_week_year(date) !== y || jet_time_iso_week(date) !== w) {
    throw new Error("ISO week date out of range");
  }
  return date;
}

function jet_time_parse_iso_week_date(value) {
  const text = String(value);
  const match = /^(-?[0-9]+)-W([0-9]{2})-([1-7])$/.exec(text);
  if (!match) throw new Error("invalid ISO week date: " + text);
  return jet_time_from_iso_week(BigInt(match[1]), BigInt(match[2]), BigInt(match[3]));
}

function jet_time_time(hour, minute, second, nanosecond) {
  const h = BigInt(hour);
  const m = BigInt(minute);
  const s = BigInt(second);
  const ns = BigInt(nanosecond === undefined ? 0n : nanosecond);
  return {
    hour: h < 0n ? 0n : h > 23n ? 23n : h,
    minute: m < 0n ? 0n : m > 59n ? 59n : m,
    second: s < 0n ? 0n : s > 59n ? 59n : s,
    nanosecond: ns < 0n ? 0n : ns > 999999999n ? 999999999n : ns,
  };
}

function jet_time_parse_time(value) {
  const text = String(value);
  const match = /^([0-9]+):([0-9]+):([0-9]+)(?:\.([0-9]{1,9}))?$/.exec(text);
  if (!match) throw new Error("invalid time: " + text);
  const fraction = match[4] === undefined ? 0n : BigInt(match[4].padEnd(9, "0"));
  const time = jet_time_time(BigInt(match[1]), BigInt(match[2]), BigInt(match[3]), fraction);
  if (BigInt(match[1]) > 23n || BigInt(match[2]) > 59n || BigInt(match[3]) > 59n) {
    throw new Error("time out of range: " + text);
  }
  return time;
}

function jet_time_time_seconds(time) {
  return BigInt(time.hour) * 3600n + BigInt(time.minute) * 60n + BigInt(time.second);
}

function jet_time_time_nanoseconds(time) {
  return jet_time_time_seconds(time) * JET_TIME_NS_SECOND + BigInt(time.nanosecond);
}

function jet_time_time_from_nanoseconds(value) {
  const normalized = jet_time_mod(value, JET_TIME_NS_DAY);
  const seconds = normalized / JET_TIME_NS_SECOND;
  return jet_time_time(
    seconds / 3600n,
    (seconds / 60n) % 60n,
    seconds % 60n,
    normalized % JET_TIME_NS_SECOND,
  );
}

function jet_time_date_time_from_ns(value) {
  const total = BigInt(value);
  return {
    secs: jet_time_clamp_i64(jet_time_floor_div(total, JET_TIME_NS_SECOND)),
    nanos: jet_time_mod(total, JET_TIME_NS_SECOND),
  };
}

function jet_time_date_time_from_ms(value) {
  return jet_time_date_time_from_ns(BigInt(value) * 1000000n);
}

function jet_time_date_time_from_seconds(value) {
  return { secs: jet_time_clamp_i64(value), nanos: 0n };
}

function jet_time_date_time_from_microseconds(value) {
  return jet_time_date_time_from_ns(BigInt(value) * 1000n);
}

function jet_time_date_time_from_nanoseconds(value) {
  return jet_time_date_time_from_ns(BigInt(value));
}

function jet_time_date_time_total_ns(value) {
  return BigInt(value.secs) * JET_TIME_NS_SECOND + BigInt(value.nanos);
}

function jet_time_date_time_date(value) {
  return jet_time_date_from_day_number(jet_time_floor_div(BigInt(value.secs), 86400n) + JET_TIME_DAY_EPOCH);
}

function jet_time_date_time_time(value) {
  const seconds = jet_time_mod(BigInt(value.secs), 86400n);
  return jet_time_time(seconds / 3600n, (seconds / 60n) % 60n, seconds % 60n, value.nanos);
}

function jet_time_date_time_add_duration(value, duration) {
  const result = jet_time_date_time_from_ns(jet_time_date_time_total_ns(value) + BigInt(duration));
  result.secs = jet_time_clamp_i64(result.secs);
  return result;
}

function jet_time_local_epoch_seconds(date, time) {
  return jet_time_clamp_i64(
    (jet_time_day_number(date) - JET_TIME_DAY_EPOCH) * 86400n + jet_time_time_seconds(time),
  );
}

function jet_time_parse_offset(value) {
  if (value === "Z") return 0n;
  const text = String(value);
  const match = /^([+-])([0-9]{2}):([0-9]{2})$/.exec(text);
  if (!match || Number(match[2]) > 23 || Number(match[3]) > 59) {
    throw new Error("bad RFC3339 offset: " + text);
  }
  const seconds = BigInt(Number(match[2]) * 3600 + Number(match[3]) * 60);
  return match[1] === "-" ? -seconds : seconds;
}

function jet_time_offset_string(offset) {
  const value = BigInt(offset);
  const absolute = value < 0n ? -value : value;
  return (value < 0n ? "-" : "+") + jet_time_pad(absolute / 3600n, 2)
    + ":" + jet_time_pad((absolute / 60n) % 60n, 2);
}

function jet_time_parse_rfc3339(value) {
  const text = String(value);
  const split = text.indexOf("T");
  if (split < 0) throw new Error("invalid RFC3339 datetime: " + text);
  const date = jet_time_date_parse(text.slice(0, split));
  const rest = text.slice(split + 1);
  let timeText;
  let offsetText;
  if (rest.endsWith("Z")) {
    timeText = rest.slice(0, -1);
    offsetText = "Z";
  } else if (rest.length >= 6) {
    timeText = rest.slice(0, -6);
    offsetText = rest.slice(-6);
  } else {
    throw new Error("RFC3339 datetime needs Z or an offset: " + text);
  }
  const time = jet_time_parse_time(timeText);
  return {
    secs: jet_time_clamp_i64(jet_time_local_epoch_seconds(date, time) - jet_time_parse_offset(offsetText)),
    nanos: BigInt(time.nanosecond),
  };
}

function jet_time_date_string(date) {
  return jet_time_year_string(date.year) + "-" + jet_time_pad(date.month, 2) + "-" + jet_time_pad(date.day, 2);
}

function jet_time_time_string(time) {
  const base = jet_time_pad(time.hour, 2) + ":" + jet_time_pad(time.minute, 2) + ":" + jet_time_pad(time.second, 2);
  return time.nanosecond === 0n ? base : base + "." + jet_time_pad(time.nanosecond, 9);
}

function jet_time_date_time_format_rfc3339(value) {
  const date = jet_time_date_time_date(value);
  const time = jet_time_date_time_time(value);
  const base = jet_time_date_string(date) + "T" + jet_time_pad(time.hour, 2) + ":"
    + jet_time_pad(time.minute, 2) + ":" + jet_time_pad(time.second, 2);
  return value.nanos === 0n ? base + "Z" : base + "." + jet_time_pad(value.nanos, 9) + "Z";
}

function jet_time_period(years, months, days) {
  return {
    years: jet_time_clamp_i64(years),
    months: jet_time_clamp_i64(months),
    days: jet_time_clamp_i64(days),
  };
}

function jet_time_period_add(left, right, sign) {
  const direction = sign === undefined ? 1n : BigInt(sign);
  return jet_time_period(
    jet_time_saturating_add_i64(left.years, direction * BigInt(right.years)),
    jet_time_saturating_add_i64(left.months, direction * BigInt(right.months)),
    jet_time_saturating_add_i64(left.days, direction * BigInt(right.days)),
  );
}

function jet_time_period_negated(period) {
  return jet_time_period(
    jet_time_saturating_negated_i64(period.years),
    jet_time_saturating_negated_i64(period.months),
    jet_time_saturating_negated_i64(period.days),
  );
}

function jet_time_period_abs(period) {
  return jet_time_period(
    jet_time_saturating_abs_i64(period.years),
    jet_time_saturating_abs_i64(period.months),
    jet_time_saturating_abs_i64(period.days),
  );
}

function jet_time_period_month_delta(period) {
  return jet_time_saturating_add_i64(
    jet_time_saturating_mul_i64(period.years, 12n),
    period.months,
  );
}

function jet_time_period_add_to_date(period, anchor) {
  return jet_time_add_days(
    jet_time_add_months(anchor, jet_time_period_month_delta(period)),
    period.days,
  );
}

function jet_time_period_add_to_datetime(period, anchor) {
  const date = jet_time_period_add_to_date(period, jet_time_date_time_date(anchor));
  const time = jet_time_date_time_time(anchor);
  const result = jet_time_date_time_from_ns(
    jet_time_local_epoch_seconds(date, time) * JET_TIME_NS_SECOND + BigInt(time.nanosecond),
  );
  result.secs = jet_time_clamp_i64(result.secs);
  return result;
}

function jet_time_period_total_in(period, unit, anchor) {
  const dateAnchor = anchor && Object.prototype.hasOwnProperty.call(anchor, "year");
  const end = dateAnchor ? jet_time_period_add_to_date(period, anchor) : jet_time_period_add_to_datetime(period, anchor);
  const startNs = dateAnchor
    ? jet_time_day_number(anchor) * JET_TIME_NS_DAY
    : jet_time_date_time_total_ns(anchor);
  const endNs = dateAnchor
    ? jet_time_day_number(end) * JET_TIME_NS_DAY
    : jet_time_date_time_total_ns(end);
  const totalNs = endNs - startNs;
  const monthDelta = jet_time_period_month_delta(period);
  const calendarDate = jet_time_add_months(
    dateAnchor ? anchor : jet_time_date_time_date(anchor),
    monthDelta,
  );
  const calendarNs = dateAnchor
    ? jet_time_day_number(calendarDate) * JET_TIME_NS_DAY
    : jet_time_date_time_total_ns(jet_time_period_add_to_datetime({ years: 0n, months: 0n, days: 0n }, {
      secs: jet_time_local_epoch_seconds(calendarDate, jet_time_date_time_time(anchor)),
      nanos: anchor.nanos,
    }));
  const residualNs = endNs - calendarNs;
  switch (String(unit)) {
    case "nanosecond":
    case "nanoseconds":
    case "ns": return Number(totalNs);
    case "microsecond":
    case "microseconds":
    case "us":
    case "µs": return Number(totalNs) / 1e3;
    case "millisecond":
    case "milliseconds":
    case "ms": return Number(totalNs) / 1e6;
    case "second":
    case "seconds":
    case "s": return Number(totalNs) / 1e9;
    case "minute":
    case "minutes":
    case "min": return Number(totalNs) / 6e10;
    case "hour":
    case "hours":
    case "h": return Number(totalNs) / 3.6e12;
    case "day":
    case "days":
    case "d": return Number(totalNs) / 8.64e13;
    case "week":
    case "weeks":
    case "w": return Number(totalNs) / 6.048e14;
    case "month":
    case "months": return Number(monthDelta) + Number(residualNs)
      / Number(jet_time_days_in_month(calendarDate.year, calendarDate.month) * JET_TIME_NS_DAY);
    case "year":
    case "years":
    case "y": return Number(monthDelta) / 12
      + Number(residualNs) / Number((jet_time_is_leap(calendarDate.year) ? 366n : 365n) * JET_TIME_NS_DAY);
    default: return 0;
  }
}

function jet_time_unit_ns(unit) {
  switch (String(unit)) {
    case "nanosecond":
    case "nanoseconds":
    case "ns": return 1n;
    case "microsecond":
    case "microseconds":
    case "us":
    case "µs": return 1000n;
    case "millisecond":
    case "milliseconds":
    case "ms": return 1000000n;
    case "second":
    case "seconds":
    case "s": return JET_TIME_NS_SECOND;
    case "minute":
    case "minutes":
    case "min": return JET_TIME_NS_MINUTE;
    case "hour":
    case "hours":
    case "h": return JET_TIME_NS_HOUR;
    case "day":
    case "days":
    case "d": return JET_TIME_NS_DAY;
    case "week":
    case "weeks":
    case "w": return JET_TIME_NS_WEEK;
    default: return null;
  }
}

function jet_time_round_quotient(value, quantum, mode) {
  const trunc = value / quantum;
  const remainder = value % quantum;
  if (remainder === 0n) return trunc;
  const away = trunc + (value < 0n ? -1n : 1n);
  const floor = value < 0n ? trunc - 1n : trunc;
  const ceil = value > 0n ? trunc + 1n : trunc;
  const absolute = remainder < 0n ? -remainder : remainder;
  switch (String(mode)) {
    case "trunc":
    case "toward_zero": return trunc;
    case "expand":
    case "away_from_zero": return away;
    case "floor": return floor;
    case "ceil": return ceil;
    case "half_trunc":
    case "halfTrunc":
    case "half-toward-zero":
      return 2n * absolute <= quantum ? trunc : away;
    case "half_expand":
    case "halfExpand":
    case "half-away-from-zero": return 2n * absolute < quantum ? trunc : away;
    case "half_even":
    case "halfEven": {
      const twice = 2n * absolute;
      if (twice < quantum) return trunc;
      if (twice > quantum) return away;
      return trunc % 2n === 0n ? trunc : away;
    }
    case "half_ceil":
    case "halfCeil":
    case "half-toward-positive": {
      const twice = 2n * absolute;
      return twice < quantum ? trunc : twice > quantum ? away : ceil;
    }
    case "half_floor":
    case "halfFloor":
    case "half-toward-negative": {
      const twice = 2n * absolute;
      return twice < quantum ? trunc : twice > quantum ? away : floor;
    }
    default: return null;
  }
}

function jet_time_round_ns(value, unit, mode, increment) {
  const quantumUnit = jet_time_unit_ns(unit);
  const step = BigInt(increment);
  if (quantumUnit === null || step <= 0n) return BigInt(value);
  const quotient = jet_time_round_quotient(BigInt(value), quantumUnit * step, mode);
  return quotient === null ? BigInt(value) : quotient * quantumUnit * step;
}

function jet_time_duration_abs(value) {
  const input = BigInt(value);
  return input === JET_I64_MIN ? JET_I64_MAX : input < 0n ? -input : input;
}

function jet_time_duration_negated(value) {
  const input = BigInt(value);
  return input === JET_I64_MIN ? JET_I64_MAX : -input;
}

function jet_time_duration_sign(value) {
  const input = BigInt(value);
  return input < 0n ? -1n : input > 0n ? 1n : 0n;
}

function jet_time_duration_total_in(value, unit) {
  const scale = jet_time_unit_ns(unit);
  return scale === null ? 0 : Number(BigInt(value)) / Number(scale);
}

function jet_time_duration_round(value, unit, increment, mode) {
  const result = jet_time_round_ns(
    BigInt(value),
    unit,
    mode === undefined ? "half_expand" : mode,
    increment === undefined ? 1n : increment,
  );
  return result < JET_I64_MIN || result > JET_I64_MAX ? BigInt(value) : result;
}

function jet_time_ok(value) {
  return { tag: "Ok", values: [value] };
}

function jet_time_err(error) {
  return { tag: "Err", values: [String(error instanceof Error ? error.message : error)] };
}

function jet_time_result(fn) {
  try {
    return jet_time_ok(fn());
  } catch (error) {
    return jet_time_err(error);
  }
}

function jet_time_text_result(fn) {
  try {
    return jet_time_ok(fn());
  } catch (error) {
    return { tag: "Err", values: [{ message: String(error instanceof Error ? error.message : error) }] };
  }
}

function jet_time_range_result(fn) {
  try {
    return jet_time_ok(fn());
  } catch (error) {
    return { tag: "Err", values: [{ reason: String(error instanceof Error ? error.message : error) }] };
  }
}

function jet_time_some(value) {
  return { tag: "Some", values: [value] };
}

function jet_time_none() {
  return { tag: "None", values: [] };
}

function jet_time_tzif_u32(bytes, offset) {
  return ((bytes[offset] << 24) | (bytes[offset + 1] << 16) | (bytes[offset + 2] << 8) | bytes[offset + 3]) >>> 0;
}

function jet_time_tzif_i64(bytes, offset) {
  let value = 0n;
  for (let index = 0; index < 8; index += 1) value = (value << 8n) | BigInt(bytes[offset + index]);
  return (value & (1n << 63n)) === 0n ? value : value - (1n << 64n);
}

function jet_time_tzif_counts(bytes, base) {
  if (String.fromCharCode.apply(null, bytes.slice(base, base + 4)) !== "TZif") {
    throw new Error("invalid tzif header");
  }
  return {
    version: String.fromCharCode(bytes[base + 4]),
    counts: Array.from({ length: 6 }, function(_, index) {
      return jet_time_tzif_u32(bytes, base + 20 + index * 4);
    }),
  };
}

function jet_time_tzif_block_size(counts, timeSize) {
  return counts[3] * timeSize + counts[3] + counts[4] * 6 + counts[5]
    + counts[2] * (timeSize + 4) + counts[1] + counts[0];
}

function jet_time_parse_tzif(name, bytes) {
  const first = jet_time_tzif_counts(bytes, 0);
  let headerBase = 0;
  let timeSize = 4;
  let parsed = first;
  if (["2", "3", "4"].includes(first.version)) {
    headerBase = 44 + jet_time_tzif_block_size(first.counts, 4);
    parsed = jet_time_tzif_counts(bytes, headerBase);
    timeSize = 8;
  }
  const counts = parsed.counts;
  let cursor = headerBase + 44;
  const transitions = [];
  for (let index = 0; index < counts[3]; index += 1) {
    const seconds = timeSize === 8
      ? jet_time_tzif_i64(bytes, cursor)
      : BigInt(jet_time_tzif_u32(bytes, cursor) | 0);
    transitions.push([seconds, 0]);
    cursor += timeSize;
  }
  const indexes = bytes.slice(cursor, cursor + counts[3]);
  cursor += counts[3];
  const infos = [];
  for (let index = 0; index < Math.max(1, counts[4]); index += 1) {
    infos.push({
      offset: BigInt(jet_time_tzif_u32(bytes, cursor) | 0),
      isDst: bytes[cursor + 4] !== 0,
    });
    cursor += 6;
  }
  for (let index = 0; index < transitions.length; index += 1) {
    transitions[index][1] = Math.min(indexes[index] === undefined ? 0 : indexes[index], infos.length - 1);
  }
  return { name: name, transitions: transitions, infos: infos };
}

function jet_time_zone(name) {
  const requested = String(name);
  if (requested === "UTC" || requested === "Etc/UTC" || requested === "Z") {
    return { name: "UTC", transitions: [], infos: [{ offset: 0n, isDst: false }] };
  }
  if (requested.includes("..") || requested.startsWith("/") || requested.startsWith("\\")) {
    throw new Error("invalid time zone name: " + requested);
  }
  if (JET_TIME_ZONE_CACHE[requested]) return JET_TIME_ZONE_CACHE[requested];
  const relative = requested.replace(/^posix\//, "").replace(/^right\//, "");
  const bytes = JET_TIME_ZONE_DATA[relative] || JET_TIME_ZONE_DATA[requested];
  if (!bytes) throw new Error("unknown IANA time zone: " + requested);
  const zone = jet_time_parse_tzif(requested, bytes);
  JET_TIME_ZONE_CACHE[requested] = zone;
  return zone;
}

function jet_time_zone_info_at_utc(zone, seconds) {
  const value = BigInt(seconds);
  let low = 0;
  let high = zone.transitions.length;
  while (low < high) {
    const middle = Math.floor((low + high) / 2);
    if (zone.transitions[middle][0] <= value) low = middle + 1;
    else high = middle;
  }
  return zone.infos[low === 0 ? 0 : zone.transitions[low - 1][1]];
}

function jet_time_zone_local_parts(zone, seconds) {
  const offset = jet_time_zone_info_at_utc(zone, seconds).offset;
  const local = jet_time_date_time_from_seconds(BigInt(seconds) + offset);
  return { date: jet_time_date_time_date(local), time: jet_time_date_time_time(local), offset: offset };
}

function jet_time_zone_local_to_utc(zone, date, time, disambiguation) {
  const policy = disambiguation === undefined ? "compatible" : String(disambiguation);
  if (policy !== "compatible" && policy !== "earlier" && policy !== "later" && policy !== "reject") {
    throw new Error("invalid disambiguation: " + policy);
  }
  const localSeconds = jet_time_local_epoch_seconds(date, time);
  const offsets = [];
  for (const info of zone.infos) {
    if (!offsets.some(function(known) { return known === info.offset; })) offsets.push(info.offset);
  }
  const candidates = [];
  for (const offset of offsets) {
    const utc = BigInt(localSeconds) - offset;
    const local = jet_time_zone_local_parts(zone, utc);
    if (local.offset === offset
      && jet_time_day_number(local.date) === jet_time_day_number(date)
      && jet_time_time_seconds(local.time) === jet_time_time_seconds(time)) {
      candidates.push(utc);
    }
  }
  candidates.sort(function(left, right) { return left < right ? -1 : left > right ? 1 : 0; });
  const unique = candidates.filter(function(value, index) {
    return index === 0 || value !== candidates[index - 1];
  });
  if (unique.length === 1) return unique[0];
  if (unique.length > 1) {
    if (policy === "compatible" || policy === "earlier") return unique[0];
    if (policy === "later") return unique[unique.length - 1];
    if (policy === "reject") throw new Error("ambiguous local time in " + zone.name);
    throw new Error("invalid disambiguation: " + policy);
  }
  for (let index = 0; index < zone.transitions.length; index += 1) {
    const transition = zone.transitions[index][0];
    const before = index === 0 ? zone.infos[0].offset : zone.infos[zone.transitions[index - 1][1]].offset;
    const after = zone.infos[zone.transitions[index][1]].offset;
    if (after > before && localSeconds >= transition + before && localSeconds < transition + after) {
      if (policy === "compatible" || policy === "later") return BigInt(localSeconds) - before;
      if (policy === "earlier") return BigInt(localSeconds) - after;
      if (policy === "reject") throw new Error("nonexistent local time in " + zone.name);
      throw new Error("invalid disambiguation: " + policy);
    }
  }
  if (policy === "reject") throw new Error("local time is not valid in " + zone.name);
  throw new Error("could not resolve local time in " + zone.name);
}

function jet_time_zone_local_to_utc_offset(zone, date, time, offset) {
  const utc = BigInt(jet_time_local_epoch_seconds(date, time)) - BigInt(offset);
  const local = jet_time_zone_local_parts(zone, utc);
  return local.offset === BigInt(offset)
    && jet_time_day_number(local.date) === jet_time_day_number(date)
    && jet_time_time_seconds(local.time) === jet_time_time_seconds(time)
    ? utc : null;
}

function jet_time_zone_start_of_day(zone, date) {
  return jet_time_zone_local_to_utc(zone, date, jet_time_time(0n, 0n, 0n), "compatible");
}

function jet_time_zone_hours_in_day(zone, date) {
  return (jet_time_zone_start_of_day(zone, jet_time_add_days(date, 1n))
    - jet_time_zone_start_of_day(zone, date)) / 3600n;
}

function jet_time_zone_transition(zone, seconds, direction) {
  const value = BigInt(seconds);
  const entries = direction === "next" ? zone.transitions : zone.transitions.slice().reverse();
  const found = entries.find(function(entry) {
    return direction === "next" ? entry[0] > value : entry[0] < value;
  });
  return found ? found[0] : null;
}

function jet_time_zoned_from_local(date, time, zone, disambiguation) {
  const seconds = jet_time_zone_local_to_utc(zone, date, time, disambiguation);
  return {
    instant: { secs: jet_time_clamp_i64(seconds), nanos: BigInt(time.nanosecond) },
    zone: zone,
  };
}

function jet_time_zoned_date(value) {
  return jet_time_zone_local_parts(value.zone, value.instant.secs).date;
}

function jet_time_zoned_time(value) {
  const time = jet_time_zone_local_parts(value.zone, value.instant.secs).time;
  return jet_time_time(time.hour, time.minute, time.second, value.instant.nanos);
}

function jet_time_replace(out, token, value) {
  return out.split(token).join(String(value));
}

function jet_time_format(pattern, date, time, zone) {
  const weekdayNames = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
  const weekdayShort = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
  const monthNames = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
  const monthShort = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  const weekdayIndex = Number(jet_time_iso_weekday(date) - 1n);
  const year = jet_time_year_string(date.year);
  const month = jet_time_pad(date.month, 2);
  const day = jet_time_pad(date.day, 2);
  const hour = jet_time_pad(time.hour, 2);
  const minute = jet_time_pad(time.minute, 2);
  const second = jet_time_pad(time.second, 2);
  const yearShort = jet_time_pad(jet_time_mod(date.year, 100n), 2);
  const dayOfYear = jet_time_pad(jet_time_day_number(date) - jet_time_day_number(jet_time_date(date.year, 1n, 1n)) + 1n, 3);
  const hour12 = jet_time_pad((BigInt(time.hour) + 11n) % 12n + 1n, 2);
  const milliseconds = jet_time_pad(BigInt(time.nanosecond) / 1000000n, 3);
  const microseconds = jet_time_pad(BigInt(time.nanosecond) / 1000n, 6);
  const nanoseconds = jet_time_pad(time.nanosecond, 9);
  const offset = zone ? jet_time_offset_string(zone.offset) : "";
  const zoneName = zone ? zone.zone.name : "";
  let out = String(pattern);
  const replacements = [
    ["%%", "%"], ["%A", weekdayNames[weekdayIndex]], ["%a", weekdayShort[weekdayIndex]],
    ["%B", monthNames[Number(date.month) - 1]], ["%b", monthShort[Number(date.month) - 1]],
    ["%Y", year], ["%y", yearShort], ["%m", month], ["%d", day],
    ["%e", jet_time_pad(date.day, 2, " ")], ["%j", dayOfYear], ["%H", hour],
    ["%I", hour12], ["%M", minute], ["%S", second], ["%p", BigInt(time.hour) < 12n ? "AM" : "PM"],
    ["%z", offset], ["%Z", zoneName], ["%F", year + "-" + month + "-" + day],
    ["%T", hour + ":" + minute + ":" + second], ["%R", hour + ":" + minute],
    ["%D", month + "/" + day + "/" + yearShort], ["%f", nanoseconds],
    ["EEEE", weekdayNames[weekdayIndex]], ["MMMM", monthNames[Number(date.month) - 1]],
    ["MMM", monthShort[Number(date.month) - 1]], ["yyyy", year], ["DDD", dayOfYear],
    ["EEE", weekdayShort[weekdayIndex]], ["MM", month], ["dd", day], ["HH", hour],
    ["mm", minute], ["ss", second], ["SSSSSSSSS", nanoseconds], ["SSSSSS", microseconds],
    ["SSS", milliseconds],
  ];
  for (const replacement of replacements) out = jet_time_replace(out, replacement[0], replacement[1]);
  if (zone) {
    out = jet_time_replace(out, "VV", zone.zone.name);
    out = jet_time_replace(out, "XXX", jet_time_offset_string(zone.offset));
  }
  return out;
}

function jet_time_format_checked(pattern, date, time, zone) {
  const text = String(pattern);
  const tokens = ["SSSSSSSSS", "EEEE", "MMMM", "yyyy", "DDD", "XXX", "SSSSSS", "MMM", "EEE", "VV", "MM", "dd", "HH", "mm", "ss", "SSS"];
  const percent = new Set(["%", "A", "a", "B", "b", "Y", "y", "m", "d", "e", "j", "H", "I", "M", "S", "p", "z", "Z", "F", "T", "R", "D", "f"]);
  const literals = [];
  let normalized = "";
  let index = 0;
  while (index < text.length) {
    const byte = text[index];
    if (byte === "'") {
      index += 1;
      let literal = "";
      let closed = false;
      while (index < text.length) {
        if (text[index] === "'") {
          if (text[index + 1] === "'") {
            literal += "'";
            index += 2;
          } else {
            index += 1;
            closed = true;
            break;
          }
        } else {
          literal += text[index];
          index += 1;
        }
      }
      if (!closed) throw new Error("E2703: unterminated format literal");
      const marker = String.fromCharCode(0xE000) + literals.length + String.fromCharCode(0xE001);
      normalized += marker;
      literals.push([marker, literal]);
      continue;
    }
    if (byte === "%") {
      const code = text[index + 1];
      if (code === undefined) throw new Error("E2703: format ends after `%`");
      if (!percent.has(code)) throw new Error("E2703: unsupported format token `%" + code + "`");
      if ((code === "z" || code === "Z") && !zone) throw new Error("E2703: format token `%" + code + "` requires a zone");
      normalized += "%" + code;
      index += 2;
      continue;
    }
    if (/[A-Za-z]/.test(byte)) {
      const token = tokens.find(function(candidate) { return text.startsWith(candidate, index); });
      if (!token) throw new Error("E2703: unsupported format token `" + byte + "`");
      if ((token === "VV" || token === "XXX") && !zone) throw new Error("E2703: format token `" + token + "` requires a zone");
      normalized += token;
      index += token.length;
      continue;
    }
    normalized += byte;
    index += 1;
  }
  let result = jet_time_format(normalized, date, time, zone);
  for (const literal of literals) result = jet_time_replace(result, literal[0], literal[1]);
  return result;
}

function jet_time_method(recv, kind, method, args) {
  const a = function(index) { return args[index]; };
  if (kind === "Date" || kind === "LocalDate") {
    switch (method) {
      case "year":
      case "month":
      case "day": return recv[method];
      case "to_string": return jet_time_date_string(recv);
      case "weekday": return jet_time_weekday(recv);
      case "iso_weekday": return jet_time_iso_weekday(recv);
      case "day_of_year": return jet_time_day_number(recv) - jet_time_day_number(jet_time_date(recv.year, 1n, 1n)) + 1n;
      case "iso_week": return jet_time_iso_week(recv);
      case "iso_week_year": return jet_time_iso_week_year(recv);
      case "quarter_of_year": return (BigInt(recv.month) - 1n) / 3n + 1n;
      case "days_in_month": return jet_time_days_in_month(recv.year, recv.month);
      case "is_leap_year": return jet_time_is_leap(recv.year);
      case "replace": return jet_time_date(a(0), a(1), a(2));
      case "add_days": return jet_time_add_days(recv, a(0));
      case "add_months": return jet_time_add_months(recv, a(0));
      case "diff_days": return jet_time_day_number(recv) - jet_time_day_number(a(0));
      case "add_period": return jet_time_period_add_to_date(a(0), recv);
      case "subtract_period": return jet_time_period_add_to_date(jet_time_period_negated(a(0)), recv);
      case "truncate": return String(a(0)) === "year" ? jet_time_date(recv.year, 1n, 1n) : String(a(0)) === "month" ? jet_time_date(recv.year, recv.month, 1n) : recv;
      case "format": return jet_time_format(a(0), recv, jet_time_time(0n, 0n, 0n), null);
      case "format_checked": return jet_time_text_result(function() { return jet_time_format_checked(a(0), recv, jet_time_time(0n, 0n, 0n), null); });
      case "until":
      case "since": {
        const delta = (method === "until" ? jet_time_day_number(a(0)) - jet_time_day_number(recv) : jet_time_day_number(recv) - jet_time_day_number(a(0))) * JET_TIME_NS_DAY;
        return jet_time_clamp_i64(jet_time_round_ns(delta, a(2), a(3), a(4)));
      }
      case "with": {
        const overflow = String(a(3));
        if (overflow === "reject" && (BigInt(a(1)) < 1n || BigInt(a(1)) > 12n || BigInt(a(2)) < 1n || BigInt(a(2)) > jet_time_days_in_month(a(0), a(1)))) throw new Error("date fields are outside the valid range");
        if (!["constrain", "clamp", "reject"].includes(overflow)) throw new Error("invalid overflow policy: " + overflow);
        return jet_time_ok(jet_time_date(a(0), a(1), a(2)));
      }
      default: throw new Error("unsupported Date method: " + method);
    }
  }
  if (kind === "LocalTime") {
    switch (method) {
      case "hour":
      case "minute":
      case "second":
      case "nanosecond": return recv[method];
      case "millisecond": return BigInt(recv.nanosecond) / 1000000n;
      case "microsecond": return BigInt(recv.nanosecond) / 1000n;
      case "to_string": return jet_time_time_string(recv);
      case "add_duration": return jet_time_time_from_nanoseconds(jet_time_time_nanoseconds(recv) + BigInt(a(0)));
      case "subtract_duration": return jet_time_time_from_nanoseconds(jet_time_time_nanoseconds(recv) + jet_time_duration_negated(a(0)));
      case "round":
      case "truncate":
      case "floor":
      case "ceil": {
        const mode = method === "round" ? a(2) : method;
        const rounded = jet_time_round_ns(jet_time_time_nanoseconds(recv), a(0), mode, a(1));
        return jet_time_time_from_nanoseconds(rounded);
      }
      case "until":
      case "since": {
        const delta = method === "until" ? jet_time_time_nanoseconds(a(0)) - jet_time_time_nanoseconds(recv) : jet_time_time_nanoseconds(recv) - jet_time_time_nanoseconds(a(0));
        return jet_time_clamp_i64(jet_time_round_ns(delta, a(2), a(3), a(4)));
      }
      case "format": return jet_time_format(a(0), jet_time_date(1970n, 1n, 1n), recv, null);
      case "format_checked": return jet_time_text_result(function() { return jet_time_format_checked(a(0), jet_time_date(1970n, 1n, 1n), recv, null); });
      default: throw new Error("unsupported LocalTime method: " + method);
    }
  }
  if (kind === "DateTime") {
    switch (method) {
      case "to_timestamp": return recv.secs;
      case "to_unix_ms": return jet_time_clamp_i64(jet_time_floor_div(jet_time_date_time_total_ns(recv), 1000000n));
      case "to_unix_s": return jet_time_clamp_i64(jet_time_floor_div(jet_time_date_time_total_ns(recv), JET_TIME_NS_SECOND));
      case "to_unix_us": return jet_time_range_result(function() { return jet_time_checked_i64(jet_time_floor_div(jet_time_date_time_total_ns(recv), 1000n), "microseconds"); });
      case "to_unix_ns": return jet_time_range_result(function() { return jet_time_checked_i64(jet_time_date_time_total_ns(recv), "nanoseconds"); });
      case "to_string": return jet_time_date_string(jet_time_date_time_date(recv)) + " " + jet_time_time_string(jet_time_date_time_time(recv)) + " UTC";
      case "date": return jet_time_date_time_date(recv);
      case "time": return jet_time_date_time_time(recv);
      case "hour":
      case "minute":
      case "second":
      case "nanosecond": return jet_time_date_time_time(recv)[method];
      case "millisecond": return BigInt(recv.nanos) / 1000000n;
      case "microsecond": return BigInt(recv.nanos) / 1000n;
      case "format_rfc3339": return jet_time_date_time_format_rfc3339(recv);
      case "format": return jet_time_format(a(0), jet_time_date_time_date(recv), jet_time_date_time_time(recv), null);
      case "format_checked": return jet_time_text_result(function() { return jet_time_format_checked(a(0), jet_time_date_time_date(recv), jet_time_date_time_time(recv), null); });
      case "plus_duration": return jet_time_date_time_add_duration(recv, a(0));
      case "subtract_duration": return jet_time_date_time_add_duration(recv, jet_time_duration_negated(a(0)));
      case "add_period": return jet_time_period_add_to_datetime(a(0), recv);
      case "subtract_period": return jet_time_period_add_to_datetime(jet_time_period_negated(a(0)), recv);
      case "difference": return jet_time_clamp_i64(jet_time_date_time_total_ns(recv) - jet_time_date_time_total_ns(a(0)));
      case "truncate":
      case "floor":
      case "ceil":
      case "round": {
        const mode = method === "round" ? a(2) : method;
        return jet_time_date_time_from_ns(jet_time_round_ns(jet_time_date_time_total_ns(recv), a(0), mode, a(1)));
      }
      case "until":
      case "since": {
        const delta = method === "until" ? jet_time_date_time_total_ns(a(0)) - jet_time_date_time_total_ns(recv) : jet_time_date_time_total_ns(recv) - jet_time_date_time_total_ns(a(0));
        return jet_time_clamp_i64(jet_time_round_ns(delta, a(2), a(3), a(4)));
      }
      case "replace": return jet_time_date_time_from_ns(jet_time_local_epoch_seconds(jet_time_date(a(0), a(1), a(2)), jet_time_time(a(3), a(4), a(5), recv.nanos)) * JET_TIME_NS_SECOND + BigInt(recv.nanos));
      case "in_zone": return { instant: { secs: recv.secs, nanos: recv.nanos }, zone: a(0) };
      case "with": {
        const overflow = String(a(6));
        if (!["constrain", "clamp", "reject"].includes(overflow)) throw new Error("invalid overflow policy: " + overflow);
        if (overflow === "reject" && (BigInt(a(3)) < 0n || BigInt(a(3)) > 23n || BigInt(a(4)) < 0n || BigInt(a(4)) > 59n || BigInt(a(5)) < 0n || BigInt(a(5)) > 59n)) throw new Error("time fields are outside the valid range");
        const date = jet_time_date(a(0), a(1), a(2));
        if (overflow === "reject" && (BigInt(a(1)) < 1n || BigInt(a(1)) > 12n || BigInt(a(2)) < 1n || BigInt(a(2)) > jet_time_days_in_month(a(0), a(1)))) throw new Error("date fields are outside the valid range");
        return jet_time_ok(jet_time_date_time_from_ns(jet_time_local_epoch_seconds(date, jet_time_time(a(3), a(4), a(5), recv.nanos)) * JET_TIME_NS_SECOND + BigInt(recv.nanos)));
      }
      default: throw new Error("unsupported DateTime method: " + method);
    }
  }
  if (kind === "Zone") {
    switch (method) {
      case "name": return recv.name;
      case "next_transition":
      case "previous_transition": {
        const transition = jet_time_zone_transition(recv, a(0), method === "next_transition" ? "next" : "previous");
        return transition === null ? jet_time_none() : jet_time_some(transition);
      }
      case "start_of_day": return { instant: jet_time_date_time_from_seconds(jet_time_zone_start_of_day(recv, a(0))), zone: recv };
      case "hours_in_day": return jet_time_zone_hours_in_day(recv, a(0));
      default: throw new Error("unsupported Zone method: " + method);
    }
  }
  if (kind === "ZonedDateTime") {
    switch (method) {
      case "date": return jet_time_zoned_date(recv);
      case "time": return jet_time_zoned_time(recv);
      case "offset_seconds": return jet_time_zone_local_parts(recv.zone, recv.instant.secs).offset;
      case "is_dst": return jet_time_zone_info_at_utc(recv.zone, recv.instant.secs).isDst;
      case "to_datetime": return { secs: recv.instant.secs, nanos: recv.instant.nanos };
      case "zone": return recv.zone;
      case "to_string": return jet_time_date_string(jet_time_zoned_date(recv)) + " " + jet_time_time_string(jet_time_zoned_time(recv)) + " " + recv.zone.name + " (" + jet_time_offset_string(jet_time_zone_local_parts(recv.zone, recv.instant.secs).offset) + ")";
      case "format": return jet_time_format(a(0), jet_time_zoned_date(recv), jet_time_zoned_time(recv), { zone: recv.zone, offset: jet_time_zone_local_parts(recv.zone, recv.instant.secs).offset });
      case "format_checked": return jet_time_text_result(function() { return jet_time_format_checked(a(0), jet_time_zoned_date(recv), jet_time_zoned_time(recv), { zone: recv.zone, offset: jet_time_zone_local_parts(recv.zone, recv.instant.secs).offset }); });
      case "format_rfc9557": {
        const offset = jet_time_zone_local_parts(recv.zone, recv.instant.secs).offset;
        return jet_time_date_string(jet_time_zoned_date(recv)) + "T" + jet_time_time_string(jet_time_zoned_time(recv)) + (offset === 0n ? "Z" : jet_time_offset_string(offset)) + "[" + recv.zone.name + "]";
      }
      case "add_duration": return { instant: jet_time_date_time_add_duration(recv.instant, a(0)), zone: recv.zone };
      case "subtract_duration": return { instant: jet_time_date_time_add_duration(recv.instant, jet_time_duration_negated(a(0))), zone: recv.zone };
      case "add_period": return jet_time_zoned_from_local(jet_time_period_add_to_date(a(0), jet_time_zoned_date(recv)), jet_time_zoned_time(recv), recv.zone);
      case "subtract_period": return jet_time_zoned_from_local(jet_time_period_add_to_date(jet_time_period_negated(a(0)), jet_time_zoned_date(recv)), jet_time_zoned_time(recv), recv.zone);
      case "with_time": return jet_time_result(function() { return jet_time_zoned_from_local(jet_time_zoned_date(recv), a(0), recv.zone, a(1)); });
      case "with_zone": return { instant: { secs: recv.instant.secs, nanos: recv.instant.nanos }, zone: a(0) };
      case "until":
      case "since": {
        const delta = method === "until" ? jet_time_date_time_total_ns(a(0).instant) - jet_time_date_time_total_ns(recv.instant) : jet_time_date_time_total_ns(recv.instant) - jet_time_date_time_total_ns(a(0).instant);
        return jet_time_clamp_i64(jet_time_round_ns(delta, a(2), a(3), a(4)));
      }
      case "next_transition":
      case "previous_transition": {
        const transition = jet_time_zone_transition(recv.zone, recv.instant.secs, method === "next_transition" ? "next" : "previous");
        return transition === null ? jet_time_none() : jet_time_some(transition);
      }
      case "start_of_day": return jet_time_method(recv.zone, "Zone", "start_of_day", [jet_time_zoned_date(recv)]);
      case "hours_in_day": return jet_time_zone_hours_in_day(recv.zone, jet_time_zoned_date(recv));
      default: throw new Error("unsupported ZonedDateTime method: " + method);
    }
  }
  if (kind === "Period") {
    switch (method) {
      case "to_string": return "P" + recv.years + "Y" + recv.months + "M" + recv.days + "D";
      case "years":
      case "months":
      case "days": return recv[method];
      case "sign": return recv.years !== 0n ? recv.years < 0n ? -1n : 1n : recv.months !== 0n ? recv.months < 0n ? -1n : 1n : recv.days < 0n ? -1n : recv.days > 0n ? 1n : 0n;
      case "is_zero": return recv.years === 0n && recv.months === 0n && recv.days === 0n;
      case "abs": return jet_time_period_abs(recv);
      case "negated": return jet_time_period_negated(recv);
      case "add": return jet_time_period_add(recv, a(0));
      case "sub": return jet_time_period_add(recv, a(0), -1n);
      case "total_in": return jet_time_period_total_in(recv, a(0), a(1));
      default: throw new Error("unsupported Period method: " + method);
    }
  }
  throw new Error("unsupported temporal receiver: " + kind);
}

function jet_time_core(method, args) {
  switch (method) {
    case "now": return BigInt(Date.now());
    case "now_utc": return jet_time_date_time_from_ms(Date.now());
    case "today": return jet_time_date_time_date(jet_time_date_time_from_ms(Date.now()));
    case "instant": return jet_time_monotonic_now_ns();
    case "utc": return jet_time_zone("UTC");
    case "new": return jet_time_date(args[0], args[1], args[2]);
    case "from_timestamp": return jet_time_date_time_from_seconds(args[0]);
    case "from_unix_ms": return jet_time_date_time_from_ms(args[0]);
    case "from_unix_seconds": return jet_time_date_time_from_seconds(args[0]);
    case "from_unix_microseconds": return jet_time_date_time_from_microseconds(args[0]);
    case "from_unix_nanoseconds": return jet_time_date_time_from_nanoseconds(args[0]);
    case "parse": return jet_time_result(function() { return jet_time_date_parse(args[0]); });
    case "parse_time": return jet_time_result(function() { return jet_time_parse_time(args[0]); });
    case "parse_rfc3339": return jet_time_result(function() { return jet_time_parse_rfc3339(args[0]); });
    case "parse_iso_week_date": return jet_time_result(function() { return jet_time_parse_iso_week_date(args[0]); });
    case "from_iso_week": return jet_time_result(function() { return jet_time_from_iso_week(args[0], args[1], args[2]); });
    case "parse_zoned": return jet_time_result(function() {
      const text = String(args[0]);
      if (!text.endsWith("]")) throw new Error("RFC9557 datetime needs a [zone]: " + text);
      const bracket = text.lastIndexOf("[");
      if (bracket < 0) throw new Error("RFC9557 datetime needs a [zone]: " + text);
      const zone = jet_time_zone(text.slice(bracket + 1, -1));
      const datetime = text.slice(0, bracket);
      const split = datetime.indexOf("T");
      if (split < 0) throw new Error("invalid RFC9557 datetime: " + text);
      const rest = datetime.slice(split + 1);
      const offsetText = rest.endsWith("Z") ? "Z" : rest.slice(-6);
      const time = jet_time_parse_time(rest.endsWith("Z") ? rest.slice(0, -1) : rest.slice(0, -6));
      const date = jet_time_date_parse(datetime.slice(0, split));
      const offset = jet_time_parse_offset(offsetText);
      const seconds = jet_time_zone_local_to_utc_offset(zone, date, time, offset);
      if (seconds === null) throw new Error("RFC9557 offset does not match zone " + zone.name);
      return { instant: { secs: jet_time_clamp_i64(seconds), nanos: BigInt(time.nanosecond) }, zone: zone };
    });
    case "datetime": return jet_time_date_time_from_ns(jet_time_local_epoch_seconds(jet_time_date(args[0], args[1], args[2]), jet_time_time(args[3], args[4], args[5])) * JET_TIME_NS_SECOND);
    case "time":
    case "local_time": return jet_time_time(args[0], args[1], args[2]);
    case "days_in_month": return jet_time_days_in_month(args[0], args[1]);
    case "is_leap_year": return jet_time_is_leap(args[0]);
    case "period": return jet_time_period(args[0], args[1], args[2]);
    case "period_days": return jet_time_period(0n, 0n, args[0]);
    case "period_months": return jet_time_period(0n, args[0], 0n);
    case "period_years": return jet_time_period(args[0], 0n, 0n);
    case "zone": return jet_time_result(function() { return jet_time_zone(args[0]); });
    case "zoned": return { instant: { secs: args[0].secs, nanos: args[0].nanos }, zone: args[1] };
    case "zoned_local": return jet_time_result(function() { return jet_time_zoned_from_local(args[0], args[1], args[2], args[3] === undefined ? "compatible" : args[3]); });
    default: throw new Error("unsupported core.time call: " + method);
  }
}

function jet_web_duration_ok(value) {
  return { tag: "Ok", values: [BigInt.asIntN(64, BigInt(value))] };
}

function jet_web_duration_err(message) {
  return { tag: "Err", values: [{ message: String(message) }] };
}

function jet_web_duration_from_int(value, scale) {
  const ns = BigInt(value) * BigInt(scale);
  return ns < JET_I64_MIN || ns > JET_I64_MAX
    ? jet_web_duration_err("duration is outside the supported range")
    : jet_web_duration_ok(ns);
}

function jet_web_duration_from_float(value, scale) {
  const ns = Number(value) * Number(scale);
  return !Number.isFinite(ns) || ns < Number(JET_I64_MIN) || ns >= 9223372036854775808
    ? jet_web_duration_err("duration must be finite and inside the supported range")
    : jet_web_duration_ok(BigInt(Math.trunc(ns)));
}

function jet_duration_scale(value, factor) {
  const result = BigInt(value) * BigInt(factor);
  if (result < JET_I64_MIN || result > JET_I64_MAX) {
    jet_runtime_stop("E3010", "", 0, "duration scaling overflowed or divided by zero");
  }
  return result;
}

function jet_duration_divide(value, factor) {
  const divisor = BigInt(factor);
  if (divisor === 0n) jet_runtime_stop("E3010", "", 0, "duration scaling overflowed or divided by zero");
  const result = BigInt(value) / divisor;
  if (result < JET_I64_MIN || result > JET_I64_MAX) {
    jet_runtime_stop("E3010", "", 0, "duration scaling overflowed or divided by zero");
  }
  return result;
}

function jet_time_add(left, right) {
  return jet_time_clamp_i64(BigInt(left) + BigInt(right));
}

function jet_time_sub(left, right) {
  return jet_time_clamp_i64(BigInt(left) - BigInt(right));
}
