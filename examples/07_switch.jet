// 07 — Choosing one branch from many: `switch` (S24).
// Each arm is a condition; the first true arm runs; `else` catches the rest.
// The backend lowers value-equality chains to a native Rust match (jump
// tables where profitable), so this is never slower than if/else.

// S25: in a `||` chain, a plain value re-applies the nearest comparison —
// `day == "sat" || "sun"` means `day == "sat" || day == "sun"`.
fn day_type(day: String) -> String {
    switch day {
        day == "sat" || "sun" -> {
            return "weekend";
        };
        day == "mon" || "tue" || "wed" || "thu" || "fri" -> {
            return "weekday";
        };
        else -> {
            return "not a day";
        };
    }
}

fn main() {
    val code = 404;
    switch code {
        code == 200 -> {
            print("ok");
        };
        code == 301 || 302 -> {
            print("redirected");
        };
        code >= 400 && code <= 499 -> {
            print("client error");
        };
        else -> {
            print("unexpected: {code}");
        };
    }

    print(day_type("sun"));
    print(day_type("wed"));
    print(day_type("pizza"));
}

// --- expected output --------------------------------------------------
// client error
// weekend
// weekday
// not a day
