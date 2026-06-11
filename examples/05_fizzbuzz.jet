// 05 — FizzBuzz, 1 through 15, with both loops (S19).
// `1..15` is inclusive — it counts 1 through 15 (S22).
// `break` and `continue` work inside both loops (S23).

// A function hands back a value with `-> Type` and `return`.
fn label(n: Int) -> String {
    if n % 15 == 0 {
        return "FizzBuzz";
    } else if n % 3 == 0 {
        return "Fizz";
    } else if n % 5 == 0 {
        return "Buzz";
    }
    return "{n}";   // interpolation turns the Int into a String
}

fn main() {
    // `for` walks a range, one number at a time
    for n in 1..15 {
        print(label(n));
    }

    // `while` repeats as long as its condition holds
    var fuel = 3;
    while fuel > 0 {
        print("t-minus {fuel}");
        fuel -= 1;
    }
    print("liftoff");
}

// --- expected output --------------------------------------------------
// 1
// 2
// Fizz
// 4
// Buzz
// Fizz
// 7
// 8
// Fizz
// Buzz
// 11
// Fizz
// 13
// 14
// FizzBuzz
// t-minus 3
// t-minus 2
// t-minus 1
// liftoff
