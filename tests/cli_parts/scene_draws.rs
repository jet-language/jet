use super::*;

const DRAWS_BUDGET: &str = r#"Budget.{
    name: "draws",
    scope: .Scene("main"),
    metric: .DrawCalls(.P99),
    provider: .SceneProbe("main"),
    comparison: .AbsoluteFrom("local/main"),
    limit: .AtMost(10),
    enforcement: .Warn,
}"#;

#[test]
fn scene_probe_draws_produces_600_samples() {
    let (_dir, report) = scene_probe_once("scene_probe_draws", DRAWS_BUDGET);
    assert_scene_probe_measurement(&report, "DrawCalls");
}
