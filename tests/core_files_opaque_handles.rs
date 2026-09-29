//! Opaque provider-owned core.files handles expose only a read-only path view.

mod common;
mod tir_support;
use jet_foundation::MIR::{
    MirArtifactBuildMode, MirArtifactKind, MirArtifactRequest, MirArtifactTarget, MirConstant,
    MirAccess, MirCallee, MirOperation, MirOwnership, MirPlaceBase, MirTerminator,
};

#[test]
fn file_handle_paths_are_read_only_provider_views_across_tiers() {
    let scratch = common::Scratch::new("core_files_opaque_handles");
    let fixture = scratch
        .path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let source = r#"
use core.files as files

fn make_writer_path(path: String) -> String {
    writer :: files.create(path) ?? panic("create")
    print(writer.path == path)
    writer.write_line("payload") ?? panic("write")
    &writer.flush() ?? panic("flush")
    writer.path
}

fn make_reader_path(path: String) -> String {
    reader :: files.open(path) ?? panic("open")
    line :: reader.read_line() ?? panic("read")
    print(line == "payload")
    reader.path
}

fn make_temp_dir_path() -> String {
    temp_dir :: files.temp_dir("opaque-handles-dir") ?? panic("temp dir")
    temp_dir.path
}

fn make_temp_file_path() -> String {
    temp_file :: files.temp_file("opaque-handles-file") ?? panic("temp file")
    temp_file.path
}

fn make_lock_path(path: String) -> String {
    lock :: files.lock(path) ?? panic("lock")
    lock.path
}

fn run() {
    root :: "__FIXTURE__"
    files.create_dir_all(root) ?? panic("root")
    path :: files.join(root, "value.txt")

    writer_path :: make_writer_path(path)
    print(writer_path == path)
    reader_path :: make_reader_path(path)
    print(reader_path == path)
    print((files.read(path) ?? panic("read")) == "payload\n")

    temp_dir_path :: make_temp_dir_path()
    print(temp_dir_path.bytes().len() > 0 && !files.exists(temp_dir_path))
    temp_file_path :: make_temp_file_path()
    print(temp_file_path.bytes().len() > 0 && !files.exists(temp_file_path))

    lock_path :: files.join(root, "value.lock")
    shown_lock_path :: make_lock_path(lock_path)
    print(shown_lock_path == lock_path)
    print(!files.exists(lock_path))
}
"#
    .replace("__FIXTURE__", &fixture);
    tir_support::assert_tiers_agree(
        "core_files_opaque_handles",
        &source,
        "true\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n",
    );
}

#[test]
fn file_owner_cleanup_tracks_lexical_exits_and_checked_transfers() {
    let source = r#"
use core.files as files

fn borrowed_path(owner: files.TempFile) -> String {
    owner.path
}

fn transfer_owner(owner: ^files.TempFile) -> files.TempFile {
    owner
}

fn return_owner(early: Bool) -> files.TempFile {
    owner :: files.temp_file("opaque-owner-return") ?? panic("temp file")
    if early {
        return owner
    }
    owner
}

fn nested_exit(early: Bool) -> String {
    if early {
        owner :: files.temp_file("opaque-owner-early") ?? panic("temp file")
        path :: owner.path
        print(files.exists(path))
        return path
    }
    owner :: files.temp_file("opaque-owner-normal") ?? panic("temp file")
    owner.path
}

fn conditional_move_path(transfer: Bool) -> String {
    owner :: files.temp_file("opaque-owner-transfer") ?? panic("temp file")
    path :: owner.path
    if transfer {
        moved :: transfer_owner(^owner)
        print(files.exists(path))
    }
    print(files.exists(path) == !transfer)
    path
}

fn run() {
    first :: return_owner(true)
    first_path :: borrowed_path(first)
    print(files.exists(first_path))
    second :: return_owner(false)
    second_path :: borrowed_path(second)
    print(files.exists(second_path))

    loop i in [0, 1] {
        path :: nested_exit(i == 0)
        print(!files.exists(path))
    }

    moved_path :: conditional_move_path(true)
    print(!files.exists(moved_path))
    kept_path :: conditional_move_path(false)
    print(!files.exists(kept_path))

    paths := [String]{}
    loop i in [0, 1] {
        if i == 0 {
            owner :: files.temp_dir("opaque-owner-nested") ?? panic("temp dir")
            path :: owner.path
            print(files.exists(path))
            &paths.push(path)
        } else {
            owner :: files.temp_file("opaque-owner-loop") ?? panic("temp file")
            path :: owner.path
            print(files.exists(path))
            &paths.push(path)
        }
    }
    loop path in paths {
        print(!files.exists(path))
    }
    print(files.exists(first_path))
    print(files.exists(second_path))
}
"#;
    let scratch = common::Scratch::new("core_files_owner_mir");
    tir_support::write_test_package(&scratch.path, tir_support::TIR_TEST_PACKAGE);
    let entry = scratch.path.join("run.jet");
    std::fs::write(&entry, source).expect("write file owner fixture");
    let mir = jet::run_compiler_work(|| {
        let mut bundle = jet::Loader::load_entry(entry.to_str().expect("UTF-8 fixture path"))
            .expect("load file owner fixture");
        let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
            .into_iter()
            .filter(|diagnostic| matches!(diagnostic.severity, jet::Diagnostics::Severity::Error))
            .collect();
        assert!(errors.is_empty(), "checked file owner fixture: {errors:#?}");
        let request = MirArtifactRequest::new(
            MirArtifactTarget::Cranelift,
            MirArtifactKind::NativeExecutable,
            MirArtifactBuildMode::Dev,
        );
        let tir = jet::Codegen::TIR::lower_checked_tir_program_for(&bundle, request)
            .expect("checked file owner TIR");
        jet::Codegen::TIR::lower_tir_to_mir(&tir).expect("file owner MIR")
    });
    let temp_file = mir.types.iter().find(|definition| {
        definition.name == "TempFile"
            && mir.modules.iter().any(|module| {
                module.id == definition.module
                    && module.path == "Core/files/files.jet"
                    && definition.key == format!("{}::TempFile", module.key)
            })
    }).expect("checked core.files TempFile type");
    let function = mir.functions.iter().find(|function| function.name == "conditional_move_path")
        .expect("conditional owner transfer function");
    let transfer = mir.functions.iter().find(|function| function.name == "transfer_owner")
        .expect("checked owned-parameter transfer function");
    assert_eq!(transfer.params[0].access, MirAccess::Move);
    assert!(
        transfer.blocks.iter().flat_map(|block| &block.instructions).any(|instruction| {
            matches!(&instruction.operation, MirOperation::MovePlace { place }
                if transfer.places.iter().any(|candidate| candidate.id == *place
                    && matches!(candidate.base, MirPlaceBase::Parameter(_))))
        }),
        "an owned imported Close parameter must be consumed into its guard, not returned as a parameter borrow",
    );
    let args = function.blocks.iter().flat_map(|block| &block.instructions)
        .find_map(|instruction| match &instruction.operation {
            MirOperation::Call { callee: MirCallee::User(callee), args, .. }
                if *callee == transfer.id => Some(args),
            _ => None,
        }).expect("checked transfer call");
    assert_eq!(args[0].access, MirAccess::Move,
        "materializing ^owner into an argument temporary must not change Move to Read");
    assert!(
        function.blocks.iter().flat_map(|block| &block.instructions).any(|instruction| {
            instruction.result == Some(args[0].value)
                && matches!(instruction.operation, MirOperation::MovePlace { .. })
        }),
        "the transfer must consume the argument temporary rather than retain its owner until caller return",
    );
    let owner = function.locals.iter().find(|local| {
        local.ty.identity == Some(temp_file.id)
    }).expect("checked TempFile owner local");
    let cleared_flag = function.blocks.iter().find_map(|block| {
        block.instructions.windows(3).find_map(|instructions| {
            let [moved, cleared, stored] = instructions else {
                return None;
            };
            if moved.result.is_some_and(|value| block.instructions.iter().any(|instruction| {
                matches!(&instruction.operation, MirOperation::Drop { value: dropped, .. } if *dropped == value)
            })) {
                return None;
            }
            if matches!(&moved.operation, MirOperation::MovePlace { place } if *place == owner.place)
                && matches!(&cleared.operation, MirOperation::Constant(MirConstant::Bool(false)))
            {
                if let MirOperation::WritePlace { place, value } = &stored.operation {
                    if Some(*value) == cleared.result {
                        return Some(*place);
                    }
                }
            }
            None
        })
    }).expect("checked owner transfer must clear its live place");
    let guarded_drop = function.blocks.iter().any(|block| {
        let MirTerminator::Branch { condition, then_target, .. } = &block.terminator else {
            return false;
        };
        let reads_live = block.instructions.iter().any(|instruction| {
            instruction.result == Some(*condition)
                && matches!(&instruction.operation,
                    MirOperation::ReadPlace(place) if *place == cleared_flag)
        });
        reads_live && function.blocks.iter().find(|candidate| candidate.id == *then_target)
            .is_some_and(|target| {
                let owner_moves = target.instructions.iter().filter_map(|instruction| {
                    match &instruction.operation {
                        MirOperation::MovePlace { place } if *place == owner.place => instruction.result,
                        _ => None,
                    }
                }).collect::<Vec<_>>();
                target.instructions.iter().any(|instruction| matches!(&instruction.operation,
                    MirOperation::Drop { value, .. } if owner_moves.contains(value)))
            })
    });
    assert!(guarded_drop, "checked TempFile owner must drop only on its live branch");
    tir_support::assert_tiers_agree(
        "core_files_owner_lifetime",
        source,
        &"true\n".repeat(16),
    );
}

#[test]
fn file_handles_have_no_fd_fields_and_cannot_be_written_or_forged() {
    let source = r#"
use core.files as files

fn run() {
    reader :: files.open("unused") ?? panic("open")
    writer :: files.create("unused") ?? panic("create")
    lock :: files.lock("unused") ?? panic("lock")
    temp_dir :: files.temp_dir("unused") ?? panic("temp dir")
    temp_file :: files.temp_file("unused") ?? panic("temp file")

    print(reader.fd)
    print(writer.fd)
    print(lock.fd)

    reader.path = "forged"
    writer.path = "forged"
    lock.path = "forged"
    temp_dir.path = "forged"
    temp_file.path = "forged"

    _ :: files.FileReader{path: "forged"}
    _ :: files.FileWriter{path: "forged"}
    _ :: files.FileLock{path: "forged"}
    _ :: files.TempDir{path: "forged"}
    _ :: files.TempFile{path: "forged"}
}
"#;
    let diagnostics = tir_support::compile_source("opaque_file_handles", source)
        .expect_err("fd access, path mutation, and handle construction must be rejected");
    for code in ["E0302", "E0339", "E0605"] {
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == code),
            "expected {code} among opaque-handle diagnostics, got {diagnostics:?}"
        );
    }
}

#[test]
fn file_manual_close_consumes_the_checked_provider_owner() {
    let scratch = common::Scratch::new("core_files_manual_close");
    let root = scratch.path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let source = r#"
use core.files as files

fn replace_closed_temp() -> String {
    owner :: files.temp_file("opaque-close-replacement") ?? panic("temp")
    path :: owner.path
    close(^owner)
    files.write(path, "replacement") ?? panic("replace temp")
    path
}

fn replace_closed_lock(path: String) {
    owner :: files.lock(path) ?? panic("lock")
    close(^owner)
    files.write(path, "replacement") ?? panic("replace lock")
}

fn run() {
    temp :: files.temp_file("opaque-manual-close") ?? panic("temp")
    temp_path :: temp.path
    print(files.exists(temp_path))
    close(^temp)
    print(!files.exists(temp_path))

    path :: files.join("__ROOT__", "reader.txt")
    writer :: files.create(path) ?? panic("create")
    writer.write_line("payload") ?? panic("write")
    &writer.flush() ?? panic("flush")
    close(^writer)
    reader :: files.open(path) ?? panic("open")
    print(reader.path == path)
    line :: reader.read_line() ?? panic("read")
    close(^reader)
    print(line == "payload")

    lock_path :: files.join("__ROOT__", "owner.lock")
    lock :: files.lock(lock_path) ?? panic("lock")
    print(files.exists(lock_path))
    close(^lock)
    print(!files.exists(lock_path))

    // The old owner's lexical exit must not delete a new ordinary file at
    // the same path after its consuming close.
    replaced_temp :: replace_closed_temp()
    print((files.read(replaced_temp) ?? panic("read replaced temp")) == "replacement")
    files.remove(replaced_temp) ?? panic("remove replaced temp")
    replaced_lock :: files.join("__ROOT__", "replacement.lock")
    replace_closed_lock(replaced_lock)
    print((files.read(replaced_lock) ?? panic("read replaced lock")) == "replacement")
    files.remove(replaced_lock) ?? panic("remove replaced lock")
}
"#.replace("__ROOT__", &root);
    tir_support::assert_tiers_agree(
        "core_files_manual_close",
        &source,
        &"true\n".repeat(8),
    );

    for (name, after_close) in [
        (
            "core_files_use_after_close_temp",
            r#"
use core.files as files
fn run() {
    owner :: files.temp_file("opaque-use-after-close") ?? panic("temp")
    close(^owner)
    print(owner.path)
}
"#,
        ),
        (
            "core_files_use_after_close_reader",
            r#"
use core.files as files
fn run() {
    reader :: files.open("unused") ?? panic("open")
    close(^reader)
    print(reader.path)
}
"#,
        ),
        (
            "core_files_use_after_close_writer",
            r#"
use core.files as files
fn run() {
    writer :: files.create("unused") ?? panic("create")
    close(^writer)
    print(writer.path)
}
"#,
        ),
    ] {
        let diagnostics = tir_support::compile_source(name, after_close)
            .expect_err("manual close must consume the source owner");
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == "E0121"),
            "use after close must be rejected by sema: {diagnostics:?}",
        );
        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic.code != "E0905"),
            "checked Core owner must implement Close: {diagnostics:?}",
        );
    }
}

#[test]
fn owned_file_replacement_releases_only_the_displaced_live_owner() {
    let scratch = common::Scratch::new("core_files_owner_replacement");
    let root = scratch.path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let source = r#"
use core.files as files

struct Holder { owner: files.TempFile }

fn transfer_owner(owner: ^files.TempFile) -> files.TempFile {
    owner
}

fn replaced_paths() -> [String] {
    owner := files.temp_file("replace-before") ?? panic("before")
    old_path :: owner.path
    owner = files.temp_file("replace-after") ?? panic("after")
    print(!files.exists(old_path))
    path :: owner.path
    print(files.exists(path))
    [old_path, path]
}

fn replace_moved_slot() {
    owner := files.temp_file("replace-moved") ?? panic("moved")
    moved :: transfer_owner(^owner)
    path :: moved.path
    owner = files.temp_file("replace-reinit") ?? panic("reinit")
    print(files.exists(path))
    close(^moved)
    print(!files.exists(path))
    replacement :: owner.path
    close(^owner)
    print(!files.exists(replacement))
}

fn failed_replacement(root: String) -> String {
    old_path :: files.join(root, "old.lock")
    owner := files.lock(old_path) ?? panic("old lock")
    occupied :: files.join(root, "occupied.lock")
    files.write(occupied, "occupied") ?? panic("occupy")
    owner = files.lock(occupied) ?? {
        print(files.exists(old_path))
        files.remove(occupied) ?? panic("remove occupied")
        return old_path
    }
    panic("lock of occupied path must fail")
}

fn replace_aggregate_slot() {
    holder := Holder{owner: files.temp_file("aggregate-before") ?? panic("before")}
    old_path :: holder.owner.path
    holder.owner = files.temp_file("aggregate-after") ?? panic("after")
    print(!files.exists(old_path))
    path :: holder.owner.path
    close(^holder.owner)
    print(!files.exists(path))
}

fn run() {
    paths :: replaced_paths()
    print(!files.exists(paths[0]))
    print(!files.exists(paths[1]))
    replace_moved_slot()
    old_lock :: failed_replacement("__ROOT__")
    print(!files.exists(old_lock))
    replace_aggregate_slot()
}
"#.replace("__ROOT__", &root);
    tir_support::assert_tiers_agree(
        "core_files_owner_replacement",
        &source,
        &"true\n".repeat(11),
    );
}

#[test]
fn aggregate_partial_moves_preserve_siblings_and_reinitialize_across_branches() {
    let source = include_str!("fixtures/core_files_partial_move_siblings.jet");
    tir_support::assert_tiers_agree(
        "core_files_partial_move_siblings",
        source,
        &"true\n".repeat(25),
    );
}

#[test]
fn moved_aggregate_parameter_drops_only_its_remaining_owned_sibling() {
    let source = r#"
use core.files as files

struct Pair { left: files.TempFile, right: files.TempFile }

fn consume(pair: ^Pair) -> String {
    left :: pair.left.path
    sibling :: pair.right.path
    close(^pair.left)
    print(!files.exists(left))
    print(files.exists(sibling))
    sibling
}

fn run() {
    pair := Pair{
        left: files.temp_file("parameter-left") ?? panic("left"),
        right: files.temp_file("parameter-right") ?? panic("right")
    }
    sibling :: consume(^pair)
    print(!files.exists(sibling))
}
"#;
    tir_support::assert_tiers_agree(
        "core_files_partial_move_parameter",
        source,
        "true\ntrue\ntrue\n",
    );
}

#[test]
fn precondition_read_temporary_does_not_release_its_callers_owners() {
    let source = r#"
use core.files as files

struct Pair { left: files.TempFile, right: files.TempFile }

#Pre(pair.left.path != pair.right.path, "distinct owners")
fn inspect(pair: Pair) {
    print(files.exists(pair.left.path))
    print(files.exists(pair.right.path))
}

fn exercise() -> String {
    pair := Pair{
        left: files.temp_file("alias-left") ?? panic("left"),
        right: files.temp_file("alias-right") ?? panic("right")
    }
    left :: pair.left.path
    right :: pair.right.path
    inspect(pair)
    print(files.exists(left))
    print(files.exists(right))
    close(^pair.left)
    print(!files.exists(left))
    print(files.exists(right))
    right
}

fn run() {
    remaining :: exercise()
    print(!files.exists(remaining))
}
"#;
    let scratch = common::Scratch::new("core_files_borrowed_temporary_mir");
    tir_support::write_test_package(&scratch.path, tir_support::TIR_TEST_PACKAGE);
    let entry = scratch.path.join("run.jet");
    std::fs::write(&entry, source).expect("write borrowed temporary fixture");
    let mir = jet::run_compiler_work(|| {
        let mut bundle = jet::Loader::load_entry(entry.to_str().expect("UTF-8 fixture path"))
            .expect("load borrowed temporary fixture");
        let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
            .into_iter()
            .filter(|diagnostic| matches!(diagnostic.severity, jet::Diagnostics::Severity::Error))
            .collect();
        assert!(errors.is_empty(), "checked borrowed temporary fixture: {errors:#?}");
        let request = MirArtifactRequest::new(
            MirArtifactTarget::Cranelift,
            MirArtifactKind::NativeExecutable,
            MirArtifactBuildMode::Dev,
        );
        let tir = jet::Codegen::TIR::lower_checked_tir_program_for(&bundle, request)
            .expect("checked borrowed temporary TIR");
        let mir = jet::Codegen::TIR::lower_tir_to_mir(&tir).expect("borrowed temporary MIR");
        jet_foundation::MIR::optimize_mir_program(
            &mir,
            &jet_foundation::MIR::MirOptimizationPolicy::conservative(),
        ).expect("contract metadata must remain canonical through optimization");
        mir
    });
    let inspect = mir.functions.iter().find(|function| function.name == "inspect")
        .expect("checked read-parameter function");
    let contract = mir.tests.iter()
        .find(|test| test.contract_generated && test.name == "contract::inspect")
        .expect("generated contract test metadata");
    let contract_target = mir.functions.iter().find(|function| function.id == contract.function)
        .expect("checked contract target");
    let parameter = contract.parameters.first().expect("sampled Pair parameter");
    let checked_parameter = contract_target.params.iter()
        .find(|candidate| candidate.index == parameter.index)
        .expect("checked contract parameter slot");
    assert_eq!(parameter.ty, checked_parameter.ty);
    assert_eq!(parameter.ty, inspect.params[0].ty);
    assert_eq!(parameter.access, checked_parameter.access);
    let exercise = mir.functions.iter().find(|function| function.name == "exercise")
        .expect("checked caller");
    let owner = exercise.locals.iter().find(|local| local.name == "pair")
        .expect("original Pair owner");
    let argument = exercise.blocks.iter().flat_map(|block| &block.instructions)
        .find_map(|instruction| match &instruction.operation {
            MirOperation::Call { callee: MirCallee::User(callee), args, .. }
                if *callee == inspect.id => args.first(),
            _ => None,
        }).expect("checked inspect argument");
    assert_eq!(argument.access, MirAccess::Read);
    let argument_place = exercise.places.iter()
        .find(|place| Some(place.id) == argument.place)
        .expect("read argument has a checked owner place");
    assert_eq!(argument_place.id, owner.place);
    assert_eq!(argument_place.base, MirPlaceBase::Local(owner.id));
    let alias = exercise.locals.iter()
        .find(|local| local.id != owner.id && local.place == owner.place)
        .expect("precondition temporary must be a genuine alias of the original owner");
    assert_eq!(alias.ownership, MirOwnership::from_access(MirAccess::Read));
    assert!(!alias.mutable, "a read borrow must not gain write access");
    assert_eq!(alias.ty, owner.ty);
    assert!(
        !exercise.places.iter().any(|place| place.base == MirPlaceBase::Local(alias.id)),
        "a read alias must not acquire independent owner storage or consuming cleanup",
    );
    tir_support::assert_tiers_agree(
        "core_files_precondition_read_temporary",
        source,
        &"true\n".repeat(7),
    );
}
