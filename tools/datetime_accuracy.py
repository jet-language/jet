#!/usr/bin/env python3
"""Generate the permanent core.time differential corpus.

The seed and case order are fixed.  Python's datetime/date/calendar/zoneinfo
modules are the independent oracle.  The generated Jet program is deliberately
boring: one bound result and one protocol line per vector.

Run ``python3 tools/datetime_accuracy.py`` to refresh the checked-in table,
witnesses, and goldens, or ``python3 tools/datetime_accuracy.py --check`` to
detect drift.
The generator and the checked-in witnesses use the repository's bundled TZif
root, so regeneration does not depend on the host's timezone package.
"""

from __future__ import annotations

import argparse
import calendar
import difflib
import random
import sys
from dataclasses import dataclass
from datetime import date, datetime, timedelta, timezone
from pathlib import Path
from zoneinfo import ZoneInfo, reset_tzpath


ROOT = Path(__file__).resolve().parents[1]
VECTOR_PATH = ROOT / "tests/fixtures/datetime_accuracy.tsv"
SOURCE_PATHS = {
    "epoch_parse": ROOT / "examples/features/time/datetime_accuracy_epoch_parse.jet",
    "civil_arithmetic": ROOT / "examples/features/time/datetime_accuracy_civil_arithmetic.jet",
    "zones": ROOT / "examples/features/time/datetime_accuracy_zones.jet",
}
GOLDEN_PATHS = {
    batch: ROOT / "examples/features/expected/time" / f"datetime_accuracy_{batch}.out"
    for batch in SOURCE_PATHS
}

SEED = 20260828
UTC = timezone.utc
EPOCH = datetime(1970, 1, 1, tzinfo=UTC)
ZONES = [
    "Europe/London",
    "America/New_York",
    "Australia/Lord_Howe",
    "Asia/Kathmandu",
    "Pacific/Apia",
    "Pacific/Chatham",
    "Africa/Casablanca",
    "America/Sao_Paulo",
    "Asia/Tehran",
    "UTC",
]
SELECTED_ARITHMETIC_ZONES = [
    "Europe/London",
    "America/New_York",
    "Australia/Lord_Howe",
    "Pacific/Chatham",
]


def configure_shared_tzpath() -> str:
    """Make Python use the committed TZif root that Jet can read everywhere."""

    path = ROOT / "corelib/tzdb"
    reset_tzpath([str(path)])
    if not (path / "Australia/Lord_Howe").is_file():
        raise RuntimeError(f"bundled TZif database is missing {path / 'Australia/Lord_Howe'}")
    return str(path)


def bool_text(value: bool) -> str:
    return "true" if value else "false"


def jet_string(value: str) -> str:
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n") + '"'


def input_text(**values: object) -> str:
    parts = []
    for key, value in values.items():
        if isinstance(value, bool):
            rendered = bool_text(value)
        else:
            rendered = str(value)
        if "\t" in rendered or "\n" in rendered or ";" in rendered:
            raise ValueError(f"vector input cannot be encoded as TSV metadata: {key}={value!r}")
        parts.append(f"{key}={rendered}")
    return ";".join(parts)


def format_epoch_ms(milliseconds: int) -> str:
    seconds, remainder = divmod(milliseconds, 1000)
    value = EPOCH + timedelta(seconds=seconds, milliseconds=remainder)
    base = value.strftime("%Y-%m-%dT%H:%M:%S")
    return f"{base}Z" if remainder == 0 else f"{base}.{remainder * 1_000_000:09d}Z"


def format_utc(value: datetime) -> str:
    value = value.astimezone(UTC)
    base = value.strftime("%Y-%m-%dT%H:%M:%S")
    nanos = value.microsecond * 1000
    return f"{base}Z" if nanos == 0 else f"{base}.{nanos:09d}Z"


def epoch_ms(value: datetime) -> int:
    value = value.astimezone(UTC)
    delta = value - EPOCH
    return delta.days * 86_400_000 + delta.seconds * 1000 + delta.microseconds // 1000


def valid_date(rng: random.Random) -> tuple[int, int, int]:
    year = rng.randint(1600, 2400)
    month = rng.randint(1, 12)
    day = rng.randint(1, calendar.monthrange(year, month)[1])
    return year, month, day


def month_add(year: int, month: int, day: int, delta: int) -> date:
    total = year * 12 + (month - 1) + delta
    new_year, month_zero = divmod(total, 12)
    new_month = month_zero + 1
    new_day = min(day, calendar.monthrange(new_year, new_month)[1])
    return date(new_year, new_month, new_day)


def aware_at_ms(milliseconds: int, zone: timezone | ZoneInfo) -> datetime:
    seconds, remainder = divmod(milliseconds, 1000)
    return (EPOCH + timedelta(seconds=seconds, milliseconds=remainder)).astimezone(zone)


def offset_text(seconds: int) -> str:
    sign = "+" if seconds >= 0 else "-"
    seconds = abs(seconds)
    return f"{sign}{seconds // 3600:02d}:{(seconds // 60) % 60:02d}"


def safe_name(value: str) -> str:
    replacements = {"+": "p", "-": "m"}
    return "".join(
        char if char.isalnum() or char == "_" else replacements.get(char, "_")
        for char in value
    ).lower()


def local_view(milliseconds: int, zone: timezone | ZoneInfo, name: str) -> tuple[str, int, bool]:
    value = aware_at_ms(milliseconds, zone)
    offset = int(value.utcoffset().total_seconds())
    dst = value.dst() not in (None, timedelta(0))
    rendered = f"{value:%Y-%m-%d %H:%M:%S} {name} {offset_text(offset)}"
    return rendered, offset, dst


def transition_seconds(zone: ZoneInfo) -> list[int]:
    start = int(datetime(2025, 12, 1, tzinfo=UTC).timestamp())
    end = int(datetime(2027, 1, 1, tzinfo=UTC).timestamp())

    def key(seconds: int) -> tuple[int, bool]:
        value = datetime.fromtimestamp(seconds, UTC).astimezone(zone)
        return int(value.utcoffset().total_seconds()), value.dst() not in (None, timedelta(0))

    found: list[int] = []
    left = start
    left_key = key(left)
    while left < end:
        right = min(left + 3600, end)
        right_key = key(right)
        if right_key != left_key:
            lo, hi = left, right
            while hi - lo > 1:
                mid = (lo + hi) // 2
                if key(mid) == left_key:
                    lo = mid
                else:
                    hi = mid
            found.append(hi)
            left_key = key(hi)
            left = hi
        else:
            left = right
    return found


def local_seconds(value: datetime) -> int:
    return int((value.replace(tzinfo=UTC) - EPOCH).total_seconds())


def offset_at_utc(seconds: int, zone: ZoneInfo) -> int:
    return int(datetime.fromtimestamp(seconds, UTC).astimezone(zone).utcoffset().total_seconds())


def jet_local_resolve(value: datetime, zone: ZoneInfo) -> int:
    """Model the current deterministic Prelude resolver for vector pinning.

    F6 also records an independent candidate-validity law.  The public time
    docs describe the instant/zone model but do not yet ratify an overlap
    choice; explicit earlier/later/reject options are owned by the later
    datetime surface work.
    """

    naive = local_seconds(value)
    guess = naive
    for _ in range(4):
        next_guess = naive - offset_at_utc(guess, zone)
        if next_guess == guess:
            break
        guess = next_guess
    return guess


def local_candidates(value: datetime, zone: ZoneInfo) -> list[int]:
    candidates: list[int] = []
    for fold in (0, 1):
        aware = value.replace(tzinfo=zone, fold=fold)
        utc_value = aware.astimezone(UTC)
        if utc_value.astimezone(zone).replace(tzinfo=None) == value:
            candidate = int(utc_value.timestamp())
            if candidate not in candidates:
                candidates.append(candidate)
    return candidates


@dataclass(frozen=True)
class Vector:
    batch: str
    family: str
    ident: str
    operation: str
    metadata: str
    oracle: str

    def protocol_line(self) -> str:
        return f"CASE\t{self.ident}\t{self.operation}\t{self.oracle}"

    def table_line(self) -> str:
        return f"{self.batch}\t{self.family}\t{self.ident}\t{self.operation}\t{self.metadata}\t{self.oracle}"


class Corpus:
    def __init__(self, batch: str) -> None:
        self.batch = batch
        self.rows: list[Vector] = []
        self.source: list[str] = []
        if batch == "epoch_parse":
            self.source.append("use core.time as time")
            self.source.extend(
                [
                    "",
                    "fn parse_status(text: String) String -> {",
                    '    return time.parse_rfc3339(text) ? value -> "accepted|{value.format_rfc3339()}" ! _error -> "rejected"',
                    "}",
                ]
            )
        elif batch == "civil_arithmetic":
            self.source.append("use core.time as date")
        else:
            self.source.append("use core.time as time")
        self.source.extend(["", "fn run() {"])
        self._value_number = 0

    def declare(self, name: str, expression: str) -> str:
        self.source.append(f"    {name} :: {expression}")
        return name

    def add(
        self,
        family: str,
        ident: str,
        operation: str,
        oracle: object,
        expression: str,
        metadata: str,
    ) -> None:
        if any(char in metadata for char in "\t\n"):
            raise ValueError(f"invalid vector metadata for {ident}")
        value = f"v_{self._value_number:04d}"
        self._value_number += 1
        self.source.append(f"    {value} :: {expression}")
        self.source.append(f'    print("CASE\\t{ident}\\t{operation}\\t{{{value}}}")')
        self.rows.append(Vector(self.batch, family, ident, operation, metadata, str(oracle)))

    def finish(self) -> None:
        self.source.append("}")


def make_epoch_and_parse(corpus: Corpus, rng: random.Random) -> tuple[list[tuple[str, str]], list[str]]:
    parse_values: list[tuple[str, str]] = []
    malformed_inputs: list[str] = []

    for index in range(150):
        milliseconds = rng.randint(-2_000_000_000_000, 4_000_000_000_000)
        ident = f"f1_{index:03d}"
        dt = corpus.declare(f"dt_f1_{index:03d}", f"time.from_unix_ms({milliseconds})")
        metadata = input_text(epoch_ms=milliseconds)
        corpus.add("F1", ident, "format", format_epoch_ms(milliseconds), f"{dt}.format_rfc3339()", metadata)
        seconds = milliseconds // 1000
        corpus.add("F1", ident, "to_timestamp", seconds, f"{dt}.to_timestamp()", metadata)
        corpus.add(
            "F1",
            ident,
            "roundtrip_ms",
            milliseconds,
            f"time.from_unix_ms({dt}.to_unix_ms()).to_unix_ms()",
            metadata,
        )

    for index in range(100):
        milliseconds = rng.randint(-2_000_000_000_000, 4_000_000_000_000)
        ident = f"f5_{index:03d}"
        dt = corpus.declare(f"dt_f5_{index:03d}", f"time.from_unix_ms({milliseconds})")
        text = corpus.declare(f"text_f5_{index:03d}", f"{dt}.format_rfc3339()")
        parsed = corpus.declare(
            f"parsed_f5_{index:03d}", f'time.parse_rfc3339({text}) ?? panic("f5 parse")'
        )
        parse_values.append((text, parsed))
        corpus.add(
            "F5",
            ident,
            "format_parse_format",
            format_epoch_ms(milliseconds),
            f"{parsed}.format_rfc3339()",
            input_text(epoch_ms=milliseconds, text=format_epoch_ms(milliseconds)),
        )

    malformed = [
        ("garbage_0", "0", "invalid"),
        ("month_13", "2024-13-01T00:00:00Z", "invalid"),
        ("day_32", "2024-01-32T00:00:00Z", "invalid"),
        ("hour_24", "2024-01-01T24:00:00Z", "invalid"),
        ("leap_second", "2024-06-30T23:59:60Z", "conditionally invalid: no leap second on chosen date"),
        ("offset_plus_24", "2024-01-01T00:00:00+24:00", "invalid"),
    ]
    for suffix, text, law in malformed:
        # Python accepts ISO-8601's 24:00 spelling, while RFC3339's hour
        # grammar is 00..23.  Keep this edge explicitly RFC3339-invalid.
        if suffix == "hour_24":
            oracle = "rejected"
        else:
            try:
                parsed = datetime.fromisoformat(text.replace("Z", "+00:00"))
                oracle = f"accepted|{format_utc(parsed)}"
            except ValueError:
                oracle = "rejected"
        value = corpus.declare(f"mal_{suffix}", jet_string(text))
        corpus.add(
            "F5",
            f"f5_malformed_{suffix}",
            "parse_status",
            oracle,
            f"parse_status({value})",
            input_text(text=text, python=oracle, rfc3339=law),
        )
        malformed_inputs.append(text)

    return parse_values, malformed_inputs


def make_civil_and_arithmetic(
    corpus: Corpus,
    rng: random.Random,
) -> tuple[list[tuple[str, str]], list[tuple[str, int, str]], list[tuple[str, int, str]]]:
    diff_pairs: list[tuple[str, str]] = []
    add_day_cases: list[tuple[str, int, str]] = []
    clamp_cases: list[tuple[str, str]] = []

    for index in range(7):
        value = date(2024, 2, 26) + timedelta(days=index)
        ident = f"cal_{index:03d}"
        d = corpus.declare(f"d_cal_{index:03d}", f"date.new({value.year}, {value.month}, {value.day})")
        metadata = input_text(date=value.isoformat(), known="weekday")
        corpus.add("CAL", ident, "weekday", (value.weekday() + 1) % 7, f"{d}.weekday()", metadata)
        corpus.add("CAL", ident, "iso_weekday", value.isoweekday(), f"{d}.iso_weekday()", metadata)

    civil = [(1900, 2, 28), (2000, 2, 29), (2100, 2, 28), (2400, 2, 29)]
    while len(civil) < 150:
        civil.append(valid_date(rng))
    for index, (year, month, day) in enumerate(civil):
        value = date(year, month, day)
        ident = f"f2_{index:03d}"
        d = corpus.declare(f"d_f2_{index:03d}", f"date.new({year}, {month}, {day})")
        metadata = input_text(year=year, month=month, day=day)
        corpus.add("F2", ident, "weekday", (value.weekday() + 1) % 7, f"{d}.weekday()", metadata)
        corpus.add("F2", ident, "iso_weekday", value.isoweekday(), f"{d}.iso_weekday()", metadata)
        corpus.add("F2", ident, "day_of_year", value.timetuple().tm_yday, f"{d}.day_of_year()", metadata)
        corpus.add("F2", ident, "iso_week", value.isocalendar().week, f"{d}.iso_week()", metadata)
        corpus.add("F2", ident, "days_in_month", calendar.monthrange(year, month)[1], f"{d}.days_in_month()", metadata)
        corpus.add("F2", ident, "is_leap_year", bool_text(calendar.isleap(year)), f"{d}.is_leap_year()", metadata)
        corpus.add("F2", ident, "quarter", (month - 1) // 3 + 1, f"{d}.quarter_of_year()", metadata)

    for index in range(60):
        year, month, day = valid_date(rng)
        delta = rng.randint(-100_000, 100_000)
        value = date(year, month, day)
        result = value + timedelta(days=delta)
        ident = f"f3_add_days_{index:03d}"
        d = corpus.declare(f"d_f3_ad_{index:03d}", f"date.new({year}, {month}, {day})")
        metadata = input_text(date=value.isoformat(), days=delta)
        corpus.add("F3", ident, "add_days", result.isoformat(), f"{d}.add_days({delta}).to_string()", metadata)
        add_day_cases.append((d, delta, value.isoformat()))

    for index in range(40):
        y1, m1, d1 = valid_date(rng)
        y2, m2, d2 = valid_date(rng)
        left = date(y1, m1, d1)
        right = date(y2, m2, d2)
        ident_left = f"f3_diff_a_{index:03d}"
        ident_right = f"f3_diff_b_{index:03d}"
        left_var = corpus.declare(f"d_f3_da_{index:03d}", f"date.new({y1}, {m1}, {d1})")
        right_var = corpus.declare(f"d_f3_db_{index:03d}", f"date.new({y2}, {m2}, {d2})")
        metadata = input_text(a=left.isoformat(), b=right.isoformat())
        corpus.add("F3", ident_left, "diff_days", (left - right).days, f"{left_var}.diff_days({right_var})", metadata)
        corpus.add("F3", ident_right, "diff_days", (right - left).days, f"{right_var}.diff_days({left_var})", metadata)
        diff_pairs.append((left_var, right_var))

    for index in range(50):
        year, month, day = valid_date(rng)
        delta = rng.randint(-2400, 2400)
        value = date(year, month, day)
        result = month_add(year, month, day, delta)
        ident = f"f3_add_months_{index:03d}"
        d = corpus.declare(f"d_f3_am_{index:03d}", f"date.new({year}, {month}, {day})")
        metadata = input_text(date=value.isoformat(), months=delta)
        corpus.add("F3", ident, "add_months", result.isoformat(), f"{d}.add_months({delta}).to_string()", metadata)

    clamp_table = [
        ("jan31_plus1", 2024, 1, 31, 1),
        ("may31_plus1", 2024, 5, 31, 1),
        ("feb29_plus12", 2024, 2, 29, 12),
        ("feb29_minus12", 2024, 2, 29, -12),
    ] + [(f"dec31_plus{index}", 2024, 12, 31, index) for index in range(1, 25)]
    for suffix, year, month, day, delta in clamp_table:
        value = date(year, month, day)
        result = month_add(year, month, day, delta)
        ident = f"f3_clamp_{suffix}"
        d = corpus.declare(f"d_clamp_{suffix}", f"date.new({year}, {month}, {day})")
        metadata = input_text(date=value.isoformat(), months=delta, table=True)
        corpus.add("F3", ident, "add_months", result.isoformat(), f"{d}.add_months({delta}).to_string()", metadata)
        clamp_cases.append((d, delta, result.isoformat()))

    for index in range(30):
        year, month, day = valid_date(rng)
        first_delta = rng.randint(-24, 24)
        second_delta = rng.randint(-24, 24)
        value = date(year, month, day)
        path = month_add(*month_add(year, month, day, first_delta).timetuple()[:3], second_delta)
        direct = month_add(year, month, day, first_delta + second_delta)
        ident = f"f3_path_{index:03d}"
        d = corpus.declare(f"d_f3_path_{index:03d}", f"date.new({year}, {month}, {day})")
        path_var = corpus.declare(f"path_f3_{index:03d}", f"{d}.add_months({first_delta}).add_months({second_delta})")
        direct_var = corpus.declare(f"direct_f3_{index:03d}", f"{d}.add_months({first_delta + second_delta})")
        metadata = input_text(date=value.isoformat(), a=first_delta, b=second_delta, divergence_allowed=True)
        result = f"path={path.isoformat()}|direct={direct.isoformat()}"
        corpus.add("F3", ident, "path_vs_direct", result, f'"path={{{path_var}.to_string()}}|direct={{{direct_var}.to_string()}}"', metadata)

    return diff_pairs, add_day_cases, clamp_cases


def make_zones_and_arithmetic(corpus: Corpus) -> None:
    zone_vars: dict[str, str] = {}
    zone_objects: dict[str, timezone | ZoneInfo] = {}
    transitions: dict[str, list[int]] = {}
    for index, name in enumerate(ZONES):
        zone_vars[name] = corpus.declare(f"zone_{index:02d}", f'time.zone("{name}") ?? panic("zone")')
        if name == "UTC":
            zone_objects[name] = UTC
            transitions[name] = []
        else:
            zone = ZoneInfo(name)
            zone_objects[name] = zone
            transitions[name] = transition_seconds(zone)

    def add_view(family: str, ident: str, milliseconds: int, name: str, tag: str, metadata: str) -> None:
        decl_name = safe_name(ident)
        dt = corpus.declare(f"dt_{decl_name}", f"time.from_unix_ms({milliseconds})")
        zoned = corpus.declare(f"z_{decl_name}", f"time.zoned({dt}, {zone_vars[name]})")
        zone = zone_objects[name]
        rendered, offset, dst = local_view(milliseconds, zone, name)
        pattern = '"yyyy-MM-dd HH:mm:ss VV XXX"'
        corpus.add(family, ident, f"{tag}_format", rendered, f"{zoned}.format({pattern})", metadata)
        corpus.add(family, ident, f"{tag}_offset", offset, f"{zoned}.offset_seconds()", metadata)
        corpus.add(family, ident, f"{tag}_dst", bool_text(dst), f"{zoned}.is_dst()", metadata)

    for name in ZONES:
        zone = zone_objects[name]
        for transition_index, transition in enumerate(transitions[name]):
            for delta in (-3600, -1, 0, 1, 3600):
                seconds = transition + delta
                ident = f"f4_{name.replace('/', '_')}_t{transition_index}_{delta:+d}"
                add_view(
                    "F4",
                    ident,
                    seconds * 1000,
                    name,
                    "view",
                    input_text(zone=name, utc_seconds=seconds, transition=transition, delta=delta),
                )
        midyear = int(datetime(2026, 6, 1, tzinfo=UTC).timestamp()) * 1000
        pre1970 = -123456789000
        for label, milliseconds in (("midyear", midyear), ("pre1970", pre1970)):
            add_view(
                "F4",
                f"f4_{name.replace('/', '_')}_{label}",
                milliseconds,
                name,
                "view",
                input_text(zone=name, utc_ms=milliseconds, baseline=label),
            )
        if name == "America/Sao_Paulo":
            historical = epoch_ms(datetime(2018, 11, 4, 3, tzinfo=UTC))
            add_view(
                "F4",
                "f4_America_Sao_Paulo_historical",
                historical,
                name,
                "view",
                input_text(zone=name, utc_ms=historical, baseline="historical"),
            )

    # The skipped Apia local date is represented by the independent Python
    # resolution, then projected with time.zoned so this corpus stays usable
    # on the current default evaluator.  F6 exercises local resolution via
    # add_period, including ambiguous fall-back times.
    apia_name = "Pacific/Apia"
    apia_zone = zone_objects[apia_name]
    requested = datetime(2011, 12, 30, 12, 0, 0)
    resolved = requested.replace(tzinfo=apia_zone, fold=0).astimezone(UTC)
    resolved_ms = epoch_ms(resolved)
    apia_ident = "f4_Pacific_Apia_skipped_day"
    add_view(
        "F4",
        apia_ident,
        resolved_ms,
        apia_name,
        "skipped",
        input_text(zone=apia_name, requested_local=requested.isoformat(), python_fold=0, resolved_utc_ms=resolved_ms),
    )

    day24 = corpus.declare("day24", 'Duration.hours(24) ?? panic("duration")')
    for name in SELECTED_ARITHMETIC_ZONES:
        zone = zone_objects[name]
        transition = transitions[name]
        if len(transition) < 2:
            raise RuntimeError(f"selected arithmetic zone has fewer than two transitions: {name}")
        bases = [transition[0] - 3600, transition[0] + 3600, transition[1] - 3600]
        for base_index, seconds in enumerate(bases):
            base_utc = datetime.fromtimestamp(seconds, UTC)
            base_local = base_utc.astimezone(zone)
            absolute_utc = base_utc + timedelta(hours=24)
            absolute = absolute_utc.astimezone(zone)
            local_target = base_local.replace(tzinfo=None) + timedelta(days=1)
            resolved_seconds = jet_local_resolve(local_target, zone)
            period = datetime.fromtimestamp(resolved_seconds, UTC).astimezone(zone)
            ident = f"f6_{name.replace('/', '_')}_{base_index}"
            decl_name = safe_name(ident)
            dt = corpus.declare(f"dt_{decl_name}", f"time.from_unix_ms({seconds * 1000})")
            zoned = corpus.declare(f"z_{decl_name}", f"time.zoned({dt}, {zone_vars[name]})")
            absolute_var = corpus.declare(f"abs_{decl_name}", f"{zoned}.add_duration({day24})")
            period_var = corpus.declare(f"period_{decl_name}", f"{zoned}.add_period(time.period_days(1))")
            metadata = input_text(zone=name, base_utc=base_utc.isoformat(), base_local=base_local.isoformat(), transition_bases=True)
            for tag, value, variable in (("duration", absolute, absolute_var), ("period", period, period_var)):
                rendered = f"{value:%Y-%m-%d %H:%M:%S} {name} {offset_text(int(value.utcoffset().total_seconds()))}"
                corpus.add("F6", ident, f"{tag}_format", rendered, f'{variable}.format("yyyy-MM-dd HH:mm:ss VV XXX")', metadata)
                corpus.add("F6", ident, f"{tag}_utc_ms", epoch_ms(value), f"{variable}.to_datetime().to_unix_ms()", metadata)
                corpus.add("F6", ident, f"{tag}_offset", int(value.utcoffset().total_seconds()), f"{variable}.offset_seconds()", metadata)
                corpus.add("F6", ident, f"{tag}_dst", bool_text(value.dst() not in (None, timedelta(0))), f"{variable}.is_dst()", metadata)

        # Force one fall-back overlap.  The exact selected instant is pinned
        # to the current Prelude resolver; validity is checked independently
        # against both Python fold candidates below.
        fall = next(
            t
            for t in transition
            if offset_at_utc(t, zone) < offset_at_utc(t - 1, zone)
        )
        before = datetime.fromtimestamp(fall - 1, UTC).astimezone(zone)
        after = datetime.fromtimestamp(fall, UTC).astimezone(zone)
        overlap_seconds = int((before.replace(tzinfo=None) - after.replace(tzinfo=None)).total_seconds())
        ambiguous = after.replace(tzinfo=None) + timedelta(seconds=overlap_seconds // 2)
        base_local = ambiguous - timedelta(days=1)
        base_seconds = int(base_local.replace(tzinfo=zone, fold=0).astimezone(UTC).timestamp())
        base_utc = datetime.fromtimestamp(base_seconds, UTC)
        target_candidates = local_candidates(ambiguous, zone)
        if len(target_candidates) != 2:
            raise RuntimeError(f"failed to find two overlap candidates for {name}")
        selected_seconds = jet_local_resolve(ambiguous, zone)
        selected = datetime.fromtimestamp(selected_seconds, UTC).astimezone(zone)
        ident = f"f6_{name.replace('/', '_')}_ambiguous_fallback"
        decl_name = safe_name(ident)
        dt = corpus.declare(f"dt_{decl_name}", f"time.from_unix_ms({base_seconds * 1000})")
        zoned = corpus.declare(f"z_{decl_name}", f"time.zoned({dt}, {zone_vars[name]})")
        absolute_var = corpus.declare(f"abs_{decl_name}", f"{zoned}.add_duration({day24})")
        period_var = corpus.declare(f"period_{decl_name}", f"{zoned}.add_period(time.period_days(1))")
        metadata = input_text(
            zone=name,
            base_utc=base_utc.isoformat(),
            base_local=base_utc.astimezone(zone).isoformat(),
            target_ambiguous_local=ambiguous.isoformat(),
            candidate_fold0_ms=target_candidates[0] * 1000,
            candidate_fold1_ms=target_candidates[1] * 1000,
            resolver_utc_ms=selected_seconds * 1000,
            python_fold=0,
        )
        absolute = (base_utc + timedelta(hours=24)).astimezone(zone)
        for tag, value, variable in (("duration", absolute, absolute_var), ("period", selected, period_var)):
            rendered = f"{value:%Y-%m-%d %H:%M:%S} {name} {offset_text(int(value.utcoffset().total_seconds()))}"
            corpus.add("F6", ident, f"{tag}_format", rendered, f'{variable}.format("yyyy-MM-dd HH:mm:ss VV XXX")', metadata)
            corpus.add("F6", ident, f"{tag}_utc_ms", epoch_ms(value), f"{variable}.to_datetime().to_unix_ms()", metadata)
            corpus.add("F6", ident, f"{tag}_offset", int(value.utcoffset().total_seconds()), f"{variable}.offset_seconds()", metadata)
            corpus.add("F6", ident, f"{tag}_dst", bool_text(value.dst() not in (None, timedelta(0))), f"{variable}.is_dst()", metadata)
        candidate0_ms, candidate1_ms = target_candidates[0] * 1000, target_candidates[1] * 1000
        corpus.add(
            "F6",
            ident,
            "period_candidate_valid",
            "true",
            f"(({period_var}.to_datetime().to_unix_ms() == {candidate0_ms}) || ({period_var}.to_datetime().to_unix_ms() == {candidate1_ms}))",
            metadata,
        )


def add_properties(
    corpus: Corpus,
    parse_values: list[tuple[str, str]],
    diff_pairs: list[tuple[str, str]],
    add_day_cases: list[tuple[str, int, str]],
    clamp_cases: list[tuple[str, int, str]],
) -> None:
    for index, (text, parsed) in enumerate(parse_values):
        corpus.add(
            "PROP",
            f"parse_format_{index:03d}",
            "parse_format_round_trip",
            "true",
            f"({parsed}.format_rfc3339() == {text})",
            input_text(source="F5", case=index),
        )
    for index, (left, right) in enumerate(diff_pairs):
        corpus.add(
            "PROP",
            f"diff_days_{index:03d}",
            "diff_days_antisymmetry",
            "true",
            f"({left}.diff_days({right}) == (0 - {right}.diff_days({left})))",
            input_text(source="F3", pair=index),
        )
    for index, (value, delta, source_date) in enumerate(add_day_cases):
        extra = 17 if index % 2 == 0 else -23
        corpus.add(
            "PROP",
            f"add_days_{index:03d}",
            "add_days_composition",
            "true",
            f"({value}.add_days({delta}).add_days({extra}).to_string() == {value}.add_days({delta + extra}).to_string())",
            input_text(source_date=source_date, first=delta, second=extra),
        )
    for index, (value, delta, expected) in enumerate(clamp_cases):
        corpus.add(
            "PROP",
            f"clamp_{index:03d}",
            "add_months_clamping",
            "true",
            f"({value}.add_months({delta}).to_string() == {jet_string(expected)})",
            input_text(source="F3 clamp", expected=expected),
        )


def build_corpora() -> tuple[list[Corpus], str]:
    tzdb = configure_shared_tzpath()
    rng = random.Random(SEED)
    epoch = Corpus("epoch_parse")
    parse_values, _ = make_epoch_and_parse(epoch, rng)
    add_properties(epoch, parse_values, [], [], [])
    epoch.finish()

    civil = Corpus("civil_arithmetic")
    diff_pairs, add_day_cases, clamp_cases = make_civil_and_arithmetic(civil, rng)
    add_properties(civil, [], diff_pairs, add_day_cases, clamp_cases)
    civil.finish()

    zones = Corpus("zones")
    make_zones_and_arithmetic(zones)
    zones.finish()
    return [epoch, civil, zones], tzdb


def render(corpora: list[Corpus]) -> tuple[str, dict[str, str], dict[str, str]]:
    rows = [row for corpus in corpora for row in corpus.rows]
    header = [
        "# datetime-accuracy-v1",
        "# generator=tools/datetime_accuracy.py",
        f"# seed={SEED}",
        "# oracle=Python stdlib datetime/date/calendar/zoneinfo; Jet and Python share the selected TZif root",
        "# families=F1 epoch;F2 civil;F3 arithmetic;F4 zones/DST;F5 parse;F6 zoned arithmetic;PROP laws",
        "# batches=epoch_parse;civil_arithmetic;zones",
        "# F6 overlap: docs/reference/core-library.md defines the instant+zone model but does not ratify a fold choice; exact resolver output is pinned and period_candidate_valid checks both independent candidates",
        "# columns=batch<TAB>family<TAB>id<TAB>operation<TAB>inputs<TAB>oracle",
    ]
    vector = "\n".join(header + [row.table_line() for row in rows]) + "\n"
    sources = {corpus.batch: "\n".join(corpus.source) + "\n" for corpus in corpora}
    goldens = {
        corpus.batch: "\n".join(row.protocol_line() for row in corpus.rows) + "\n"
        for corpus in corpora
    }
    return vector, sources, goldens


def check_or_write(check: bool) -> int:
    corpora, tzdb = build_corpora()
    vector, sources, goldens = render(corpora)
    expected = {VECTOR_PATH: vector}
    expected.update({SOURCE_PATHS[batch]: source for batch, source in sources.items()})
    expected.update({GOLDEN_PATHS[batch]: golden for batch, golden in goldens.items()})
    if check:
        failed = False
        for path, content in expected.items():
            actual = path.read_text() if path.is_file() else ""
            if actual == content:
                continue
            failed = True
            print(f"drift: {path.relative_to(ROOT)} (shared TZif root: {tzdb})", file=sys.stderr)
            print(
                "".join(
                    difflib.unified_diff(
                        actual.splitlines(keepends=True),
                        content.splitlines(keepends=True),
                        fromfile=str(path),
                        tofile=f"{path} (regenerated)",
                    )
                ),
                file=sys.stderr,
            )
        return 1 if failed else 0

    for path, content in expected.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
    print(f"generated {len(corpora)} witnesses and {sum(len(corpus.rows) for corpus in corpora)} vectors with seed {SEED} using {tzdb}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail when checked-in artifacts differ")
    args = parser.parse_args()
    return check_or_write(args.check)


if __name__ == "__main__":
    raise SystemExit(main())
