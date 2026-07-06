use jet::Comptime::Build::{
    ActionCache, ActionSpec, BuildCapability, BuildContext, BuildError, TargetKind, TargetSpec,
};

#[test]
fn registers_typed_targets_and_default_plan() {
    let mut b = BuildContext::new();
    let gen = b
        .action(
            "generate-assets",
            ActionSpec::cached(["jet-tool", "pack"])
                .with_inputs(["assets/sprite.png"])
                .with_outputs([".jet/generated/assets.jet"])
                .with_cap(BuildCapability::Fs),
        )
        .unwrap();
    let lib = b
        .add_library(
            "engine",
            TargetSpec::new()
                .with_source("src/engine.jet")
                .with_action(gen),
        )
        .unwrap();
    let app = b
        .add_executable(
            "game",
            TargetSpec::new().with_source("src/main.jet").with_dep(lib),
        )
        .unwrap();
    b.add_test("unit", TargetSpec::new().with_dep(app)).unwrap();
    b.add_bench("frame-time", TargetSpec::new().with_dep(app))
        .unwrap();
    b.add_asset_bundle("assets", TargetSpec::new().with_action(gen))
        .unwrap();
    b.add_doc("manual", TargetSpec::new().with_source("docs/manual.md"))
        .unwrap();
    b.add_install("install", TargetSpec::new().with_dep(app))
        .unwrap();
    b.add_package("package", TargetSpec::new().with_dep(app))
        .unwrap();
    b.add_publish("publish", TargetSpec::new().with_dep(app))
        .unwrap();

    let plan = b.plan_with_default(app).unwrap();

    assert_eq!(plan.default_target(), Some(app.into()));
    assert_eq!(plan.target(app).unwrap().kind, TargetKind::Executable);
    assert_eq!(plan.target(lib).unwrap().kind, TargetKind::Library);
    assert_eq!(plan.targets_by_kind(TargetKind::AssetBundle).len(), 1);
    assert_eq!(plan.actions().len(), 1);
    assert_eq!(
        plan.action(gen).unwrap().outputs[0].as_str(),
        ".jet/generated/assets.jet"
    );
}

#[test]
fn duplicate_target_or_action_names_are_rejected() {
    let mut b = BuildContext::new();
    b.add_executable("app", TargetSpec::new()).unwrap();
    let err = b.add_library("app", TargetSpec::new()).unwrap_err();
    assert_eq!(err, BuildError::DuplicateTargetName("app".to_string()));

    b.action(
        "codegen",
        ActionSpec::cached(["tool"]).with_outputs(["out.jet"]),
    )
    .unwrap();
    let err = b
        .action(
            "codegen",
            ActionSpec::cached(["tool"]).with_outputs(["other.jet"]),
        )
        .unwrap_err();
    assert_eq!(err, BuildError::DuplicateActionName("codegen".to_string()));
}

#[test]
fn cached_actions_need_outputs() {
    let mut b = BuildContext::new();
    let err = b
        .action("probe", ActionSpec::cached(["cc", "--version"]))
        .unwrap_err();
    assert_eq!(
        err,
        BuildError::CachedActionWithoutOutputs("probe".to_string())
    );
}

#[test]
fn outputless_commands_are_explicit_uncached_phony_actions() {
    let mut b = BuildContext::new();
    let clean = b
        .action(
            "clean-generated",
            ActionSpec::uncached_phony(["rm", "-rf", ".jet/generated"])
                .with_cap(BuildCapability::Fs),
        )
        .unwrap();

    let plan = b.plan().unwrap();
    let action = plan.action(clean).unwrap();
    assert_eq!(action.cache, ActionCache::UncachedPhony);
    assert!(action.outputs.is_empty());
    assert_eq!(plan.phony_actions(), vec![action]);
}

#[test]
fn phony_actions_need_a_declared_capability() {
    let mut b = BuildContext::new();
    let err = b
        .action("clean-generated", ActionSpec::uncached_phony(["rm", "-rf", ".jet/generated"]))
        .unwrap_err();
    assert_eq!(
        err,
        BuildError::PhonyActionWithoutCaps("clean-generated".to_string())
    );
}

#[test]
fn action_outputs_have_one_owner_in_the_plan() {
    let mut b = BuildContext::new();
    b.action(
        "generate-a",
        ActionSpec::cached(["tool-a"]).with_outputs([".jet/generated/out.jet"]),
    )
    .unwrap();
    b.action(
        "generate-b",
        ActionSpec::cached(["tool-b"]).with_outputs([".jet/generated/out.jet"]),
    )
    .unwrap();

    let err = b.plan().unwrap_err();
    assert_eq!(
        err,
        BuildError::DuplicateBuildOutput {
            output: ".jet/generated/out.jet".to_string(),
            first_action: "generate-a".to_string(),
            second_action: "generate-b".to_string(),
        }
    );
}
