const SIDE: usize = 4096;
const ELEMENTS: usize = SIDE * SIDE;

fn main() {
    let left = vec![1.0_f64; ELEMENTS];
    let right = vec![2.0_f64; ELEMENTS];
    let mut mapped = Vec::with_capacity(ELEMENTS);
    for (a, b) in left.iter().zip(&right) {
        mapped.push(*a + *b);
    }

    let corner = mapped[ELEMENTS - 1];
    let checksum = mapped.iter().copied().sum::<f64>() as u64;
    assert_eq!(corner, 3.0);
    assert_eq!(checksum, 50_331_648);
    println!(
        "shape:[4096, 4096] numel:{ELEMENTS} corner:{corner:.1} checksum:{checksum}"
    );
}
