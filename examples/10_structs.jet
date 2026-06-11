// 10 — Structs with methods (M3, S27/S29).

struct Point {
    x: Float;
    y: Float;

    fn dist_sq(self) -> Float {
        return self.x * self.x + self.y * self.y;
    }

    fn unit() -> Point {
        return Point { x: 1.0, y: 0.0 };
    }
}

fn main() {
    val p = Point { x: 3.0, y: 4.0 };
    print(p.dist_sq());
    val u = Point.unit();
    print(u.x);
}
