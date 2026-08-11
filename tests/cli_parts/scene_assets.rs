use super::*;

const ASSETS_BUDGET: &str = r#"Budget.{
    name: "assets",
    scope: .Scene("main"),
    metric: .SceneAssetBytes,
    provider: .SceneProbe("main"),
    comparison: .AbsoluteFrom("local/main"),
    limit: .AtMost(1MiB),
    enforcement: .Warn,
}"#;

#[test]
fn scene_probe_assets_produces_600_samples() {
    let (_dir, report) = scene_probe_once("scene_probe_assets", ASSETS_BUDGET);
    assert_scene_probe_measurement(&report, "SceneAssetBytes");
}
