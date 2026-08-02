use super::Boundary::NativeBoundary;

#[test]
fn private_integration_has_pinned_product_ready_evaluator() {
    let boundary = NativeBoundary::embedded().expect("committed manifest must validate");
    assert!(boundary.product_ready());
}
