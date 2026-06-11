// 13 — Recursive enum with invisible boxing (M3).

enum Expr {
    Num(Int);
    Wrap(Expr);
}

fn main() {
    val e = Expr.Wrap(Expr.Num(1));
    print(e);
}
