use super::*;

const MEMORY_BUDGET: &str = r#"Budget.{
    name: "memory",
    scope: .Scene("main"),
    metric: .MemoryHighWater,
    provider: .SceneProbe("main"),
    comparison: .AbsoluteFrom("local/main"),
    limit: .AtMost(256MiB),
    enforcement: .Warn,
}"#;

#[test]
fn scene_probe_memory_produces_600_samples() {
    let (_dir, report) = scene_probe_once("scene_probe_memory", MEMORY_BUDGET);
    assert_scene_probe_measurement(&report, "MemoryHighWater");
}
