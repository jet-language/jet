// 06 — Compound assignment (S17): `n += 1` means `n = n + 1`.
// `+=` `-=` `*=` `/=` `%=` work on Int and Float (`%=` Int-only);
// `&=` `|=` `^=` `<<=` `>>=` are Int-only bit operations.
// The left side must be a `var` — compound-assigning a `val` is an error.

// Multi-argument calls (M1): arguments separated by commas.
fn show(label: String, num: Int) {
    print("{label} = {num}");
}

fn main() {
    var n = 100;
    n += 20;
    show("after += 20", n);
    n -= 45;
    show("after -= 45", n);
    n *= 2;
    show("after *= 2", n);
    n /= 4;
    show("after /= 4", n);    // Int division drops the remainder
    n %= 10;
    show("after %= 10", n);

    n &= 6;
    show("after &= 6", n);
    n |= 9;
    show("after |= 9", n);
    n ^= 5;
    show("after ^= 5", n);
    n <<= 2;
    show("after <<= 2", n);
    n >>= 3;
    show("after >>= 3", n);

    var price = 9.5;
    price *= 2.0;
    print("price = {price}");
}

// --- expected output --------------------------------------------------
// after += 20 = 120
// after -= 45 = 75
// after *= 2 = 150
// after /= 4 = 37
// after %= 10 = 7
// after &= 6 = 6
// after |= 9 = 15
// after ^= 5 = 10
// after <<= 2 = 40
// after >>= 3 = 5
// price = 19.0
