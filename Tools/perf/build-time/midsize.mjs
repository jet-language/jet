#!/usr/bin/env node
// Deterministic generator for the mid-size build-time workload.
//
// One shape, four languages: every unit declares the same record, the same
// three-case sum type, and the same classify/score/step/unit functions with
// identical integer arithmetic, and `run`/`main` folds every unit into one
// checksum line. The Jet source is ~5k lines; the peers are the idiomatic
// spelling of the same program in Go, Zig, and Rust. All values stay
// non-negative and below 2^40, so `%` means the same thing everywhere and no
// language can overflow or trap. The manifest pins the SHA-256 of every
// generated source and of the shared expected output, so any generator
// change fails the build-time gate until the manifest is updated deliberately.
//
// usage: node Tools/perf/build-time/midsize.mjs OUT_DIR
// writes OUT_DIR/{main.jet,main.go,main.zig,main.rs}

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const UNITS = 90;
const MODULUS = 1000003;

function unitConstants(i) {
  const t1 = 200 + ((i * 17) % 300);
  return {
    c1: 3 + ((i * 7) % 17),
    c2: 5 + ((i * 11) % 23),
    c3: 101 + ((i * 13) % 97),
    t1,
    t2: t1 + 300 + ((i * 19) % 200),
    n: 12 + (i % 5),
  };
}

function jetUnit(i) {
  const { c1, c2, c3, t1, t2, n } = unitConstants(i);
  return `// Unit ${i}: state step, classification, and scoring.
struct State${i} {
    a: Int
    b: Int
    c: Int
}

enum Kind${i} {
    Low(Int)
    Mid(Int)
    High
}

fn classify_${i}(x: Int) -> Kind${i} {
    if x < ${t1} {
        return Kind${i}.Low(x)
    }
    if x < ${t2} {
        return Kind${i}.Mid(x - ${t1})
    }
    return Kind${i}.High
}

fn score_${i}(kind: Kind${i}) -> Int {
    return if kind == {
        .Low(v) -> v * ${c1} + 1
        .Mid(v) -> v * ${c2} + 7
        .High -> ${c3}
    }
}

fn step_${i}(s: State${i}, x: Int) -> State${i} {
    a :: (s.a * ${c1} + x + 1) % ${MODULUS}
    b :: (s.b + a * ${c2}) % ${MODULUS}
    c :: (s.c + a + b * 3) % ${MODULUS}
    return State${i}{a: a, b: b, c: c}
}

fn unit_${i}(seed: Int) -> Int {
    s := State${i}{a: seed % ${MODULUS}, b: ${c2}, c: ${c3}}
    values := [Int]{}
    loop j in 0..<${n} {
        s = step_${i}(s, j)
        &values.push(score_${i}(classify_${i}(s.a % 1000)))
    }
    total := 0
    loop v in values {
        if v % 2 == 0 {
            total += v
        } else {
            total += v * 3
        }
    }
    return (total + s.b + s.c) % ${MODULUS}
}
`;
}

function goUnit(i) {
  const { c1, c2, c3, t1, t2, n } = unitConstants(i);
  return `// Unit ${i}: state step, classification, and scoring.
type State${i} struct {
	a int64
	b int64
	c int64
}

const (
	kind${i}Low = iota
	kind${i}Mid
	kind${i}High
)

type Kind${i} struct {
	tag int
	v   int64
}

func classify${i}(x int64) Kind${i} {
	if x < ${t1} {
		return Kind${i}{tag: kind${i}Low, v: x}
	}
	if x < ${t2} {
		return Kind${i}{tag: kind${i}Mid, v: x - ${t1}}
	}
	return Kind${i}{tag: kind${i}High}
}

func score${i}(kind Kind${i}) int64 {
	switch kind.tag {
	case kind${i}Low:
		return kind.v*${c1} + 1
	case kind${i}Mid:
		return kind.v*${c2} + 7
	default:
		return ${c3}
	}
}

func step${i}(s State${i}, x int64) State${i} {
	a := (s.a*${c1} + x + 1) % ${MODULUS}
	b := (s.b + a*${c2}) % ${MODULUS}
	c := (s.c + a + b*3) % ${MODULUS}
	return State${i}{a: a, b: b, c: c}
}

func unit${i}(seed int64) int64 {
	s := State${i}{a: seed % ${MODULUS}, b: ${c2}, c: ${c3}}
	values := []int64{}
	for j := int64(0); j < ${n}; j++ {
		s = step${i}(s, j)
		values = append(values, score${i}(classify${i}(s.a%1000)))
	}
	var total int64
	for _, v := range values {
		if v%2 == 0 {
			total += v
		} else {
			total += v * 3
		}
	}
	return (total + s.b + s.c) % ${MODULUS}
}
`;
}

function zigUnit(i) {
  const { c1, c2, c3, t1, t2, n } = unitConstants(i);
  return `// Unit ${i}: state step, classification, and scoring.
const State${i} = struct {
    a: i64,
    b: i64,
    c: i64,
};

const Kind${i} = union(enum) {
    low: i64,
    mid: i64,
    high: void,
};

fn classify${i}(x: i64) Kind${i} {
    if (x < ${t1}) {
        return .{ .low = x };
    }
    if (x < ${t2}) {
        return .{ .mid = x - ${t1} };
    }
    return .high;
}

fn score${i}(kind: Kind${i}) i64 {
    return switch (kind) {
        .low => |v| v * ${c1} + 1,
        .mid => |v| v * ${c2} + 7,
        .high => ${c3},
    };
}

fn step${i}(s: State${i}, x: i64) State${i} {
    const a = @mod(s.a * ${c1} + x + 1, ${MODULUS});
    const b = @mod(s.b + a * ${c2}, ${MODULUS});
    const c = @mod(s.c + a + b * 3, ${MODULUS});
    return .{ .a = a, .b = b, .c = c };
}

fn unit${i}(allocator: std.mem.Allocator, seed: i64) !i64 {
    var s = State${i}{ .a = @mod(seed, ${MODULUS}), .b = ${c2}, .c = ${c3} };
    var values: std.ArrayList(i64) = .empty;
    defer values.deinit(allocator);
    var j: i64 = 0;
    while (j < ${n}) : (j += 1) {
        s = step${i}(s, j);
        try values.append(allocator, score${i}(classify${i}(@mod(s.a, 1000))));
    }
    var total: i64 = 0;
    for (values.items) |v| {
        if (@mod(v, 2) == 0) {
            total += v;
        } else {
            total += v * 3;
        }
    }
    return @mod(total + s.b + s.c, ${MODULUS});
}
`;
}

function rustUnit(i) {
  const { c1, c2, c3, t1, t2, n } = unitConstants(i);
  return `// Unit ${i}: state step, classification, and scoring.
struct State${i} {
    a: i64,
    b: i64,
    c: i64,
}

enum Kind${i} {
    Low(i64),
    Mid(i64),
    High,
}

fn classify_${i}(x: i64) -> Kind${i} {
    if x < ${t1} {
        return Kind${i}::Low(x);
    }
    if x < ${t2} {
        return Kind${i}::Mid(x - ${t1});
    }
    Kind${i}::High
}

fn score_${i}(kind: Kind${i}) -> i64 {
    match kind {
        Kind${i}::Low(v) => v * ${c1} + 1,
        Kind${i}::Mid(v) => v * ${c2} + 7,
        Kind${i}::High => ${c3},
    }
}

fn step_${i}(s: &State${i}, x: i64) -> State${i} {
    let a = (s.a * ${c1} + x + 1) % ${MODULUS};
    let b = (s.b + a * ${c2}) % ${MODULUS};
    let c = (s.c + a + b * 3) % ${MODULUS};
    State${i} { a, b, c }
}

fn unit_${i}(seed: i64) -> i64 {
    let mut s = State${i} { a: seed % ${MODULUS}, b: ${c2}, c: ${c3} };
    let mut values: Vec<i64> = Vec::new();
    for j in 0..${n} {
        s = step_${i}(&s, j);
        values.push(score_${i}(classify_${i}(s.a % 1000)));
    }
    let mut total: i64 = 0;
    for v in &values {
        if v % 2 == 0 {
            total += v;
        } else {
            total += v * 3;
        }
    }
    (total + s.b + s.c) % ${MODULUS}
}
`;
}

const units = (render) => Array.from({ length: UNITS }, (_, i) => render(i)).join("\n");
const seed = (i) => i * 37 + 1;

export function generate(language) {
  const header = "Mid-size build-time workload (generated by Tools/perf/build-time/midsize.mjs; do not edit).";
  if (language === "jet") {
    const folds = Array.from({ length: UNITS }, (_, i) => `    total = (total * 31 + unit_${i}(${seed(i)})) % ${MODULUS}`).join("\n");
    return `// ${header}\n${units(jetUnit)}\nfn run() {\n    total := 0\n${folds}\n    print("checksum {total}")\n}\n`;
  }
  if (language === "go") {
    const folds = Array.from({ length: UNITS }, (_, i) => `\ttotal = (total*31 + unit${i}(${seed(i)})) % ${MODULUS}`).join("\n");
    return `// ${header}\npackage main\n\nimport "fmt"\n\n${units(goUnit)}\nfunc main() {\n\tvar total int64\n${folds}\n\tfmt.Printf("checksum %d\\n", total)\n}\n`;
  }
  if (language === "zig") {
    const folds = Array.from({ length: UNITS }, (_, i) => `    total = @mod(total * 31 + try unit${i}(allocator, ${seed(i)}), ${MODULUS});`).join("\n");
    return `// ${header}\nconst std = @import("std");\n\n${units(zigUnit)}\npub fn main(init: std.process.Init) !void {\n    const allocator = std.heap.page_allocator;\n    var total: i64 = 0;\n${folds}\n    var out_buffer: [64]u8 = undefined;\n    var out = std.Io.File.Writer.init(.stdout(), init.io, &out_buffer);\n    try out.interface.print("checksum {d}\\n", .{total});\n    try out.interface.flush();\n}\n`;
  }
  if (language === "rust") {
    const folds = Array.from({ length: UNITS }, (_, i) => `    total = (total * 31 + unit_${i}(${seed(i)})) % ${MODULUS};`).join("\n");
    return `// ${header}\n${units(rustUnit)}\nfn main() {\n    let mut total: i64 = 0;\n${folds}\n    println!("checksum {total}");\n}\n`;
  }
  throw new Error(`unknown midsize language: ${language}`);
}

export const EXTENSIONS = { jet: "jet", go: "go", zig: "zig", rust: "rs" };

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const out = process.argv[2];
  if (!out) {
    console.error("usage: node Tools/perf/build-time/midsize.mjs OUT_DIR");
    process.exit(64);
  }
  fs.mkdirSync(out, { recursive: true });
  for (const [language, extension] of Object.entries(EXTENSIONS)) {
    fs.writeFileSync(path.join(out, `main.${extension}`), generate(language));
  }
}
