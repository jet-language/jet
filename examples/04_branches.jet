// 04 — Making decisions with if / else if / else.
// Comparison: == != < > <= >=   Logic: && || !   (S13)
// Parameters carry no access keyword in M1 — plain shared read (S10).

fn describe(celsius: Float) {
    val freezing = celsius <= 0.0;
    val pleasant = celsius >= 18.0 && celsius <= 26.0;

    if freezing {
        print("{celsius} C: bundle up");
    } else if pleasant {
        print("{celsius} C: just right");
    } else if celsius > 35.0 || celsius < -20.0 {
        print("{celsius} C: stay inside");
    } else {
        print("{celsius} C: fine");
    }
}

fn main() {
    describe(-5.0);
    describe(21.0);
    describe(40.0);
    describe(10.0);

    val raining = false;
    if !raining {
        print("no umbrella needed");
    }
}

// --- expected output --------------------------------------------------
// (S21: a Float always prints with a decimal part)
// -5.0 C: bundle up
// 21.0 C: just right
// 40.0 C: stay inside
// 10.0 C: fine
// no umbrella needed
