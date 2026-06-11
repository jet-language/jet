// Syntax showcase — a small grade-book program.
//
// Previews ratified/provisional surface syntax from docs/02-syntax-decisions.md.
// NOT meant to compile until M1–M6; read this file for look-and-feel only.
//
//   S1 fn   S2 val / var   S3 { } blocks   S4 name: Type   S5 //
//   S6 ;    S7 ?           S8 "hi {name}"   S9 print        S12 main
//   S10 read / write / take   S11 Int, Float, Bool, String
//   S13 && || ! == != < > <= >=   S18 pub (private by default)
//   S17 += -= *= /= %= &= |= ^= <<= >>=
//   S14 one spelling (write `fn`, not `def`)   S16 import "path" as alias

import "grades/scoring" as scoring;

// --- ownership on parameters (S10, M2) -------------------------------
// private helpers — visible in this file only

fn show(read label: String, read value: Int) {
    print("{label}: {value}");
}

fn bump(write score: Int) {
    score += 1;
}

fn archive(take name: String) -> String {
    return name;
}

// --- errors (S7, M4) -------------------------------------------------

fn parse_score(read raw: String) -> Result<Int, String> {
    return scoring.parse_int(raw)?;
}

// --- values & control flow (M1) --------------------------------------

fn print_report(read name: String, score: Int) {
    val grade: String = scoring.letter_grade(score);
    val passed: Bool = score >= 60 && !(score > 100 || score < 0);

    if !passed {
        print("needs improvement");
        return;
    }

    if grade == "A" || grade == "B" {
        print("report for {name}: {score} points, grade {grade}");
    } else if grade != "F" {
        print("{name} passed with {grade}");
    }
}

// --- entry point (S12) — pub so the runtime can find it --------------

pub fn main() {
    var attempts: Int = 0;
    val name: String = "Alex";
    val raw: String = "88";

    val score: Int = parse_score(raw)?;
    attempts += 1;

    show("attempts", attempts);
    bump(attempts);
    print_report(name, score);

    val backup: String = archive("saved copy");
    print(backup);
}
