#[path = "target/bindings/projection_rust.rs"]
mod projection;

fn main() {
    let initial = [10_u8, 20, 30];
    let replacement = [40_u8, 50, 60];
    let mut document = projection::ResourceDocument::open(&initial).unwrap();
    let stale = document.bytes().unwrap();
    assert_eq!(document.at(&stale, 1), Ok(20));
    document.replace(&replacement).unwrap();
    assert_eq!(
        document.at(&stale, 1),
        Err(projection::PROJECTION_RUST_EXPIRED_VIEW)
    );
    let fresh = document.bytes().unwrap();
    assert_eq!(document.at(&fresh, 1), Ok(50));
    assert_eq!(document.close(), Ok(()));
    assert_eq!(
        document.close(),
        Err(projection::PROJECTION_RUST_CLOSED)
    );
    assert_eq!(unsafe { projection::add(41) }, 42);
}
