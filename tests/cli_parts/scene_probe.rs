use super::*;

const FRAME_BUDGET: &str = r#"Budget.{
    name: "frame",
    scope: .Scene("main"),
    metric: .FrameTime(.P99),
    provider: .SceneProbe("main"),
    comparison: .AbsoluteFrom("local/main"),
    limit: .AtMost(16ms),
    enforcement: .Warn,
}"#;

#[test]
fn scene_probe_reuses_compatible_report() {
    let (dir, report) = scene_probe_once("scene_probe_runtime", FRAME_BUDGET);
    assert_scene_probe_measurement(&report, "FrameTime");
    let reports = dir.join(".jet/perf/reports");
    let paths = || fs::read_dir(&reports).unwrap().map(|entry| entry.unwrap().path()).collect::<Vec<_>>();
    let initial = paths();
    assert_eq!(initial.len(), 1, "expected exactly one report; got {:?}", initial);

    // Second run should reuse cached report (compatible identity → no new report).
    let second = scene_probe_run(&dir);
    assert_eq!(second.status.code(), Some(0));
    assert_eq!(paths(), initial, "compatible report should be reused");
}
