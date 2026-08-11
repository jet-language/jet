use super::*;

const FRAME_BUDGET: &str = r#"Budget.{
    name: "assets_forged",
    scope: .Scene("main"),
    metric: .SceneAssetBytes,
    provider: .SceneProbe("main"),
    comparison: .AbsoluteFrom("local/main"),
    limit: .AtMost(1MiB),
    enforcement: .Warn,
}"#;

#[test]
fn scene_probe_rejects_forged_cache() {
    let (dir, report) = scene_probe_once("scene_probe_forged", FRAME_BUDGET);
    assert_scene_probe_measurement(&report, "SceneAssetBytes");
    let reports = dir.join(".jet/perf/reports");
    let paths = || fs::read_dir(&reports).unwrap().map(|entry| entry.unwrap().path()).collect::<Vec<_>>();
    let initial = paths();
    assert_eq!(initial.len(), 1, "expected exactly one report; got {:?}", initial);

    fs::OpenOptions::new().append(true).open(&initial[0]).unwrap().write_all(b"forged").unwrap();
    let replacement = scene_probe_run(&dir);
    assert_eq!(replacement.status.code(), Some(0), "{}", String::from_utf8_lossy(&replacement.stderr));
    assert_eq!(paths().len(), 2, "forged report must not satisfy compatible cache identity");
}
