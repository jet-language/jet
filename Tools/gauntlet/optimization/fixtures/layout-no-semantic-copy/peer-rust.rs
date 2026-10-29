#[repr(C)]
struct Pair {
    left: u32,
    right: u32,
}

fn main() {
    let pair = Pair { left: 1, right: 2 };
    println!("{}", pair.left + pair.right);
}
