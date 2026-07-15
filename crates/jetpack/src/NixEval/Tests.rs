use super::Boundary::{InternalStage, NativeBoundary};

#[test]
fn partial_stage_has_only_test_harness_authority() {
    let boundary = NativeBoundary::embedded().expect("committed manifest must validate");
    let harness = boundary.internal_test_harness();
    assert_eq!(harness.engine(), "native-jetpack");
    for stage in [
        InternalStage::Syntax,
        InternalStage::Values,
        InternalStage::Evaluation,
        InternalStage::Authority,
        InternalStage::Derivation,
        InternalStage::Flakes,
    ] {
        assert_eq!(boundary.authorize_internal(&harness, stage).stage(), stage);
    }
    assert!(!boundary.product_ready());
}
