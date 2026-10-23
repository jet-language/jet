//! D-MARKER-GROUP1=A, D-MARKER-MODULE1=A, D-CODEGEN-HINTS1=A.
//! These tests exercise the loader and ordinary marker checking, not a second
//! expansion implementation. The same Jet inputs are usable by compiler-diff.

mod common;

use jet::AST::{Item, Marker, ProgramBundle};
use jet::Diagnostics::Diagnostic;
use jet::Sema::MarkerModules::{declaration_by_span, resolve_marker_application, MarkerResolution};

fn checked(files: &[(&str, &str)]) -> (Option<ProgramBundle>, Vec<Diagnostic>) {
    let scratch = common::Scratch::new("marker_groups_modules");
    if files.iter().any(|(name, source)| *name == "main.jet" && source.starts_with("package {"))
        && !files.iter().any(|(name, _)| *name == "package.jet")
    {
        std::fs::remove_file(scratch.join("package.jet")).unwrap();
    }
    for (name, source) in files {
        let path = scratch.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
    match jet::Loader::load_entry(scratch.join("main.jet").to_str().unwrap()) {
        Ok(mut bundle) => {
            let mut diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Check);
            diagnostics.extend(std::mem::take(&mut bundle.parse_teaching));
            (Some(bundle), diagnostics)
        }
        Err(diagnostics) => (None, diagnostics),
    }
}

fn clean(files: &[(&str, &str)]) -> ProgramBundle {
    let (bundle, diagnostics) = checked(files);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    bundle.expect("accepted bundle")
}

fn rejects(files: &[(&str, &str)], code: &str) {
    let (_, diagnostics) = checked(files);
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == code),
        "expected {code}: {diagnostics:?}");
    assert!(diagnostics.iter().all(|diagnostic| jet_foundation::Registry::diagnostic(&diagnostic.code).is_some()),
        "every diagnostic must be registered: {diagnostics:?}");
}

fn function<'a>(items: &'a [Item], name: &str) -> &'a jet::AST::Func {
    items.iter().find_map(|item| match item {
        Item::Func(function) if function.name == name => Some(function),
        _ => None,
    }).unwrap_or_else(|| panic!("missing function {name}"))
}

fn custom_marker<'a>(markers: &'a [Marker], name: &str) -> &'a Marker {
    markers.iter().find(|marker| marker.name == name || marker.name.ends_with(&format!(".{name}")))
        .unwrap_or_else(|| panic!("missing marker {name}: {markers:?}"))
}

#[test]
fn same_module_marker_is_bare_and_imports_use_the_local_alias() {
    let bundle = clean(&[
        ("main.jet", "use library as lib\nmarker LocalNote($sites: [.Function])\n#[LocalNote, lib.LibraryNote(\"direct\")] fn helper() {}\nfn run() {}\n"),
        ("library.jet", "marker LibraryNote(label: String, $sites: [.Function])\n"),
    ]);
    let helper = function(&bundle.modules[bundle.entry].items, "helper");
    let local = custom_marker(&helper.markers, "LocalNote");
    let imported = custom_marker(&helper.markers, "LibraryNote");
    assert_eq!(local.name, "LocalNote");
    assert_eq!(imported.name, "lib.LibraryNote");
    let MarkerResolution::Found(local_row) = resolve_marker_application(&bundle, bundle.entry, local) else { panic!("local marker must resolve") };
    let MarkerResolution::Found(imported_row) = resolve_marker_application(&bundle, bundle.entry, imported) else { panic!("imported marker must resolve") };
    assert_eq!(local_row.module, bundle.entry);
    assert_ne!(imported_row.module, bundle.entry);
    assert_eq!(imported.declaration, Some((imported_row.module, imported_row.declaration.name_span)));
    assert_eq!(declaration_by_span(&bundle.modules[imported_row.module].items, imported_row.declaration.name_span).unwrap().name, "LibraryNote");
}

#[test]
fn bare_imported_unknown_private_and_ambiguous_names_have_distinct_errors() {
    let library = "marker LibraryNote($sites: [.Function])\nmarker Hidden($sites: [.Function])\n";
    for (application, code) in [
        ("#LibraryNote", "E0085"),
        ("#lib.Missing", "E0927"),
        ("#Missing", "E0927"),
        ("#lib.Hidden", "E0605"),
    ] {
        let main = format!("use library as lib\n{application} fn helper() {{}}\nfn run() {{}}\n");
        rejects(&[("main.jet", &main), ("library.jet", library)], code);
    }
    rejects(&[
        ("main.jet", include_str!("ui/marker_module_bare/main.jet")),
        ("library.jet", include_str!("ui/marker_module_bare/library.jet")),
    ], "E0085");
    rejects(&[
        ("main.jet", "use first as lib\nuse second as lib\n#lib.LibraryNote fn helper() {}\nfn run() {}\n"),
        ("first.jet", library),
        ("second.jet", library),
    ], "E0086");
    rejects(&[
        ("main.jet", include_str!("ui/marker_module_ambiguous/main.jet")),
        ("first.jet", include_str!("ui/marker_module_ambiguous/first.jet")),
    ], "E0086");
}

#[test]
fn exported_nested_marker_and_group_keep_their_declaration_identity() {
    let bundle = clean(&[
        ("main.jet", "use library as lib\n#lib.inner.Fast fn helper(value: Int) -> Int { value + 1 }\nfn run() {}\n"),
        ("library.jet", "module inner {\nmarker LibraryNote($sites: [.Function])\nmarker Fast = #[LibraryNote, Inline]\n}\n"),
    ]);
    let helper = function(&bundle.modules[bundle.entry].items, "helper");
    assert!(helper.is_inline);
    let marker = custom_marker(&helper.markers, "LibraryNote");
    let MarkerResolution::Found(resolved) = resolve_marker_application(&bundle, bundle.entry, marker) else { panic!("nested member must resolve") };
    assert_ne!(resolved.module, bundle.entry);
    assert_eq!(resolved.declaration.name, "LibraryNote");
    assert!(resolved.exported);
    assert_eq!(marker.declaration, Some((resolved.module, resolved.declaration.name_span)));
    assert!(declaration_by_span(&bundle.modules[resolved.module].items, resolved.declaration.name_span).is_some());
}

#[test]
fn imported_group_binds_members_and_fixed_arguments_in_the_declaring_module() {
    let bundle = clean(&[
        ("main.jet", "use library as lib\nuse other as rules\nmarker LibraryNote(label: Int, $sites: [.Function])\n#rules.LibraryNote(7) fn direct() {}\n#lib.Fast fn helper() {}\nfn run() {}\n"),
        ("library.jet", "use rules\nmarker Fast = #[rules.LibraryNote(\"fixed\"), Inline]\n"),
        ("rules.jet", "marker LibraryNote(label: String, $sites: [.Function])\n"),
        ("other.jet", "marker LibraryNote(label: Int, $sites: [.Function])\n"),
    ]);
    let helper = function(&bundle.modules[bundle.entry].items, "helper");
    assert!(helper.is_inline);
    let marker = custom_marker(&helper.markers, "LibraryNote");
    let MarkerResolution::Found(resolved) = resolve_marker_application(&bundle, bundle.entry, marker) else { panic!("lexically bound member must resolve") };
    assert_eq!(bundle.modules[resolved.module].path.file_name().unwrap(), "rules.jet");
    assert_eq!(resolved.declaration.params[0].name, "label");
    assert_eq!(marker.declaration, Some((resolved.module, resolved.declaration.name_span)));
}

#[test]
fn local_group_expands_before_ordinary_applicability_and_argument_checks() {
    let bundle = clean(&[("main.jet", "marker Fast = #[Inline(.Always), Scalar]\n#Fast fn helper(value: Int) -> Int { value + 1 }\nfn run() {}\n")]);
    let helper = function(&bundle.modules[bundle.entry].items, "helper");
    assert!(helper.is_inline_always);
    assert!(!helper.is_inline);
    assert!(helper.markers.iter().any(|marker| marker.name == "Scalar"));
    assert!(helper.markers.iter().all(|marker| marker.name != "Fast"));
    rejects(&[("main.jet", "marker WrongArgs = #[Inline(.Other), Scalar]\n#WrongArgs fn helper() {}\nfn run() {}\n")], "E0930");
    rejects(&[("main.jet", "marker WrongSite = #[Inline, Scalar]\n#WrongSite struct Bad { value: Int }\nfn run() {}\n")], "E0355");
    // The declaration alone is legal; its target and argument checks are deferred.
    clean(&[("main.jet", "marker Unused = #[Inline(.Other), Scalar]\nfn run() {}\n")]);
}

#[test]
fn overlapping_groups_and_direct_members_share_the_duplicate_check() {
    for (source, count) in [
        ("marker Fast = #[Inline, Scalar]\n#[Fast, Inline] fn helper() {}\nfn run() {}\n", 1),
        ("marker Fast = #[Inline, Scalar]\nmarker Other = #[Inline, MustUse]\n#[Fast, Other] fn helper() -> Int { return 1 }\nfn run() {}\n", 1),
        ("marker Fast = #[Inline, Inline, Inline]\n#Fast fn helper() {}\nfn run() {}\n", 2),
    ] {
        let (_, diagnostics) = checked(&[("main.jet", source)]);
        assert_eq!(diagnostics.iter().filter(|diagnostic| diagnostic.code == "E0999").count(), count,
            "each repeated application reports once: {diagnostics:?}");
    }
    rejects(&[
        ("main.jet", "use library as lib\n#[lib.Fast, lib.LibraryNote] fn helper() {}\nfn run() {}\n"),
        ("library.jet", "marker LibraryNote($sites: [.Function])\nmarker Fast = #[LibraryNote, Inline]\n"),
    ], "E0999");
}

#[test]
fn groups_require_two_members_no_parameters_and_no_hidden_forbidden_kinds() {
    for source in [
        "marker Empty = #[]\nfn run() {}\n",
        "marker Single = #[Inline]\nfn run() {}\n",
    ] {
        rejects(&[("main.jet", source)], "E0999");
    }
    for member in ["Unsafe(\"audit\")", "Impure(\"audit\")", "Nondeterministic(\"audit\")", "FX(.Net)", "Scrub(Input)", "allow(float_money)", "!Debug", "InlineAlways", "Missing"] {
        let source = format!("marker Bad = #[{member}, Inline]\nfn run() {{}}\n");
        rejects(&[("main.jet", &source)], "E0400");
    }
    rejects(&[("main.jet", "marker First = #[Inline, Scalar]\nmarker Second = #[First, MustUse]\nfn run() {}\n")], "E0400");
    rejects(&[
        ("main.jet", "use library as lib\nmarker Second = #[lib.First, MustUse]\nfn run() {}\n"),
        ("library.jet", "marker First = #[Inline, Scalar]\n"),
    ], "E0400");
    let (_, diagnostics) = checked(&[("main.jet", "marker Bad(value: Int) = #[Inline, Scalar]\nfn run() {}\n")]);
    assert!(!diagnostics.is_empty(), "a named group must not accept a signature");
    rejects(&[("main.jet", "marker Fast = #[Inline, Scalar]\n#Fast(1) fn helper() {}\nfn run() {}\n")], "E0930");
    rejects(&[("main.jet", "marker Fast = #[Inline, Scalar]\n#Fast() fn helper() {}\nfn run() {}\n")], "E0930");
}

#[test]
fn public_groups_cannot_publish_private_members() {
    rejects(&[("main.jet", "marker Hidden($sites: [.Function])\npub marker Public = #[Hidden, Inline]\nfn run() {}\n")], "E0605");
    rejects(&[
        ("main.jet", "use library\nfn run() {}\n"),
        ("library.jet", "use rules\npub marker Public = #[rules.Hidden, Inline]\n"),
        ("rules.jet", "marker Hidden($sites: [.Function])\n"),
    ], "E0605");
    clean(&[("main.jet", "marker Hidden($sites: [.Function])\nmarker Private = #[Hidden, Inline]\n#Private fn helper() {}\nfn run() {}\n")]);
}

#[test]
fn formatter_preserves_authored_groups_and_qualified_names() {
    let source = "use library as lib\nmarker Fast = #[Inline(.Always), Scalar]\n#Fast fn helper() {}\n#lib.LibraryNote(\"fixed\") fn imported() {}\nfn run() {}\n";
    let formatted = jet::format_source(source).expect("groups and qualified markers format");
    assert!(formatted.contains("marker Fast = #[Inline(.Always), Scalar]"), "{formatted}");
    assert!(formatted.contains("#Fast"), "{formatted}");
    assert!(formatted.contains("#lib.LibraryNote(\"fixed\")"), "{formatted}");
    assert_eq!(jet::format_source(&formatted).unwrap(), formatted);
}

#[test]
fn codegen_controls_are_four_separate_registry_rows_and_projections() {
    use jet_foundation::Policy::{applied_rule, RuleSite};
    for name in ["Inline", "Scalar", "Kernel", "Static"] {
        assert_eq!(applied_rule(name).expect("separate registered row").name, name);
    }
    assert!(applied_rule("Static").unwrap().sites.contains(&RuleSite::Constant));
    assert!(!applied_rule("Static").unwrap().sites.contains(&RuleSite::Function));
    assert!(!applied_rule("Kernel").unwrap().sites.contains(&RuleSite::Constant));
    let bundle = clean(&[("main.jet", "#Inline fn hint(value: Int) -> Int { value + 1 }\n#Inline(.Always) fn promise(value: Int) -> Int { value + 1 }\n#Scalar fn scalar(value: Int) -> Int { value + 1 }\n#Kernel(.parallel) fn kernel(value: Int) -> Int { value + 1 }\n#Static TABLE :: comptime { 7 }\nfn run() { print(TABLE) }\n")]);
    let items = &bundle.modules[bundle.entry].items;
    assert!(function(items, "hint").is_inline);
    assert!(!function(items, "hint").is_inline_always);
    assert!(function(items, "promise").is_inline_always);
    assert!(!function(items, "promise").is_inline);
    let scalar = function(items, "scalar");
    assert!(!scalar.is_inline && !scalar.is_inline_always && scalar.kernel.is_none());
    assert!(scalar.markers.iter().any(|marker| marker.name == "Scalar"));
    let kernel = function(items, "kernel");
    assert!(!kernel.is_inline && !kernel.is_inline_always);
    assert!(kernel.kernel.as_ref().and_then(|marker| marker.proof.as_ref()).is_some_and(|proof| proof.is_complete()));
}

#[test]
fn core_import_alias_resolves_a_loader_materialized_marker_declaration() {
    // This is a resolver fixture, not a new Core API: attach an ordinary
    // declaration to the real loader-created core.compiler.lang source module.
    let scratch = common::Scratch::new("marker_core_import");
    std::fs::write(scratch.join("main.jet"),
        "use core.compiler.lang as lang\n#lang.Fixture fn helper() {}\nfn run() {}\n").unwrap();
    let mut bundle = jet::Loader::load_entry(scratch.join("main.jet").to_str().unwrap()).unwrap();
    let core = jet_foundation::CoreModuleExports::core_source_module("core.compiler.lang").unwrap();
    let module = bundle.modules.iter().position(|module| module.alias == core.alias)
        .expect("Core import materializes its canonical source module");
    let (tokens, diagnostics) = jet::Lexer::lex("marker Fixture($sites: [.Function])");
    assert!(diagnostics.is_empty());
    let mut declaration = jet::Parser::parse(&tokens).unwrap().items;
    bundle.modules[module].items.append(&mut declaration);
    let helper = function(&bundle.modules[bundle.entry].items, "helper");
    let marker = custom_marker(&helper.markers, "Fixture");
    let MarkerResolution::Found(resolved) = resolve_marker_application(&bundle, bundle.entry, marker) else {
        panic!("Core alias must use the same declaration resolver as a library alias")
    };
    assert_eq!(resolved.module, module);
    assert!(resolved.exported);
    assert_eq!(resolved.declaration.name, "Fixture");
}

#[test]
fn imported_group_replay_preserves_loader_synthesized_module_imports() {
    let bundle = clean(&[
        ("main.jet", "module helper\nuse library as lib\n#lib.Fast fn work() {}\nfn run() { print(helper.value()) }\n"),
        ("helper.jet", "fn value() -> Int { 7 }\n"),
        ("library.jet", "marker Fast = #[Inline, Scalar]\n"),
    ]);
    let main = &bundle.modules[bundle.entry];
    assert!(main.imports.iter().any(|import| import.import_alias() == "helper"),
        "group replay must retain the import synthesized for `module helper`");
    assert!(function(&main.items, "work").is_inline);
}

#[test]
fn imported_group_replay_preserves_inline_package_masking() {
    let bundle = clean(&[
        ("main.jet", "package { name: \"marker_replay\", version: \"0.1.0\" }\nuse library as lib\n#lib.Fast pub fn work() {}\nfn run() {}\n"),
        ("library.jet", "pub marker Fast = #[Inline, Scalar]\n"),
    ]);
    let main = &bundle.modules[bundle.entry];
    assert!(main.source.starts_with("package {"));
    assert!(main.frontend_source.as_ref().is_some_and(|source| !source.starts_with("package {")),
        "replay retains the authored source separately from the masked frontend source");
}

#[test]
fn a_qualified_marker_with_a_builtin_prefix_does_not_apply_that_builtin() {
    for alias in ["Inline", "Test", "HTML", "PubFile", "Policy"] {
        let source = format!("use library as {alias}\n#{alias}.LibraryNote pub fn helper() {{}}\nfn run() {{}}\n");
        // Parser projections must not mistake a qualified head for a builtin.
        // Sema separately rejects these aliases under the current case rules.
        let (tokens, diagnostics) = jet::Lexer::lex(&source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let program = jet::Parser::parse_for_fmt(&tokens).unwrap();
        let helper = function(&program.items, "helper");
        assert!(!helper.is_inline && !helper.is_inline_always && !helper.is_job);
        assert_eq!(custom_marker(&helper.markers, "LibraryNote").name, format!("{alias}.LibraryNote"));
        assert!(!program.pub_file && program.html_path.is_none());
        rejects(&[
            ("main.jet", &source),
            ("library.jet", "pub marker LibraryNote($sites: [.Function])\n"),
        ], "E0357");
        let formatted = jet::format_source(&source).unwrap();
        assert!(formatted.contains(&format!("#{alias}.LibraryNote")), "{formatted}");
    }
}

#[test]
fn every_custom_member_expanded_at_one_span_keeps_its_target_and_signature() {
    let bundle = clean(&[("main.jet",
        "marker First(value: String, $sites: [.Function])\nmarker Second(value: Int, $sites: [.Function])\nmarker Pair = #[First(\"fixed\"), Second(7)]\n#Pair pub fn helper() {}\nfn run() {}\n")]);
    let main = &bundle.modules[bundle.entry];
    let facts: Vec<_> = main.rule_facts.iter()
        .filter(|fact| matches!(fact.marker.name.as_str(), "First" | "Second"))
        .collect();
    assert_eq!(facts.len(), 2, "{facts:?}");
    assert!(facts.iter().all(|fact| fact.target.is_some()), "{facts:?}");
    assert!(facts.iter().all(|fact| fact.site == Some(jet::Policy::RuleSite::Function)), "{facts:?}");
    assert_eq!(facts[0].marker.span, facts[1].marker.span,
        "the two members are expanded at the one authored group application");
    for source in [
        "marker First(value: String, $sites: [.Function])\nmarker Second(value: Int, $sites: [.Function])\nmarker Pair = #[First(7), Second(7)]\n#Pair fn helper() {}\nfn run() {}\n",
        "marker First(value: String, $sites: [.Function])\nmarker Second(value: Int, $sites: [.Function])\nmarker Pair = #[First(\"fixed\"), Second(\"wrong\")]\n#Pair fn helper() {}\nfn run() {}\n",
    ] {
        rejects(&[("main.jet", source)], "E0930");
    }
    rejects(&[("main.jet",
        "marker First($sites: [.Function])\nmarker Second($sites: [.Type])\nmarker Pair = #[First, Second]\n#Pair fn helper() {}\nfn run() {}\n")], "E0355");
}

#[test]
fn nested_local_group_members_shadow_root_rows_and_outer_groups_are_inherited() {
    clean(&[("main.jet",
        "marker LibraryNote(value: Int, $sites: [.Function])\nmodule outer {\nmarker LibraryNote(value: String, $sites: [.Function])\nmarker Fast = #[LibraryNote(\"outer\"), Inline]\n#Fast pub fn direct() {}\nmodule inner {\n#Fast pub fn inherited() {}\n}\n}\nfn run() {}\n")]);
    clean(&[("main.jet",
        "marker LibraryNote(value: Int, $sites: [.Function])\nmarker Fast = #[LibraryNote(7), Scalar]\nmodule inner {\nmarker LibraryNote(value: String, $sites: [.Function])\nmarker Fast = #[LibraryNote(\"inner\"), Inline]\n#Fast pub fn direct() {}\n}\nfn run() {}\n")]);
}

#[test]
fn nested_import_aliases_bind_groups_locally_and_do_not_fall_through_when_shadowed() {
    clean(&[
        ("main.jet", "use first as rules\n#rules.LibraryNote(1) pub fn root() {}\nmodule inner {\nuse second as rules\nmarker Fast = #[rules.LibraryNote(\"nested\"), Inline]\n#Fast pub fn helper() {}\n}\nfn run() {}\n"),
        ("first.jet", "pub marker LibraryNote(value: Int, $sites: [.Function])\n"),
        ("second.jet", "pub marker LibraryNote(value: String, $sites: [.Function])\n"),
    ]);
    rejects(&[
        ("main.jet", "use first as rules\nmodule inner {\nuse second as rules\n#rules.LibraryNote pub fn helper() {}\n}\nfn run() {}\n"),
        ("first.jet", "pub marker LibraryNote($sites: [.Function])\n"),
        ("second.jet", "pub marker Other($sites: [.Function])\n"),
    ], "E0927");
}

#[test]
fn imported_group_replay_preserves_the_package_auto_derive_default() {
    let bundle = clean(&[
        ("package.jet", "name: \"marker_replay\"\nversion: \"1.0.0\"\npolicy: { lints: { deny: [auto_derive] } }\n"),
        ("main.jet", "use library as lib\n#lib.Fast pub fn helper() {}\nstruct Plain { value: Int }\nfn run() {}\n"),
        ("library.jet", "pub marker Fast = #[Inline, Scalar]\n"),
    ]);
    let plain = bundle.modules[bundle.entry].items.iter().find_map(|item| match item {
        Item::Struct(definition) if definition.name == "Plain" => Some(definition),
        _ => None,
    }).expect("replay retains the authored struct");
    assert!(!plain.auto_derive_default,
        "replaying a group use must not restore the parser's pre-package default");
}

#[test]
fn local_file_group_projects_flags_before_struct_and_marker_declarations() {
    let bundle = clean(&[("main.jet",
        "marker Defaults = #[PubFile, NoPrelude]\n#Defaults\nstruct Plain {}\nmarker LibraryNote($sites: [.Function])\n#LibraryNote fn helper() {}\nfn run() {}\n")]);
    let main = &bundle.modules[bundle.entry];
    assert!(main.pub_file && main.no_prelude);
    let marker = custom_marker(&function(&main.items, "helper").markers, "LibraryNote");
    let MarkerResolution::Found(resolved) = resolve_marker_application(&bundle, bundle.entry, marker) else {
        panic!("file-default marker declaration must resolve")
    };
    assert!(resolved.exported);
}

#[test]
fn imported_file_group_establishes_transitive_marker_export_visibility() {
    let bundle = clean(&[
        ("main.jet", "use library as lib\n#lib.LibraryNote pub fn direct() {}\n#lib.Fast pub fn grouped() {}\nfn run() {}\n"),
        ("library.jet", "use flags\n#flags.Defaults\nmarker LibraryNote($sites: [.Function])\nmarker Fast = #[LibraryNote, Inline]\nstruct Plain {}\n"),
        ("flags.jet", "pub marker Defaults = #[PubFile, NoPrelude]\n"),
    ]);
    let library = bundle.modules.iter().position(|module| module.path.file_name().unwrap() == "library.jet")
        .expect("imported library");
    assert!(bundle.modules[library].pub_file && bundle.modules[library].no_prelude);
    assert!(function(&bundle.modules[bundle.entry].items, "grouped").is_inline);
    let marker = custom_marker(&function(&bundle.modules[bundle.entry].items, "direct").markers, "LibraryNote");
    let MarkerResolution::Found(resolved) = resolve_marker_application(&bundle, bundle.entry, marker) else {
        panic!("imported file defaults must publish its marker declaration")
    };
    assert_eq!(resolved.module, library);
    assert!(resolved.exported);
}

#[test]
fn grouped_file_visibility_does_not_export_another_modules_private_member() {
    rejects(&[
        ("main.jet", "use library\nfn run() {}\n"),
        ("library.jet", "use flags\nuse rules\n#flags.Defaults\nmarker Public = #[rules.Hidden, Inline]\n"),
        ("flags.jet", "pub marker Defaults = #[PubFile, NoPrelude]\n"),
        ("rules.jet", "marker Hidden($sites: [.Function])\n"),
    ], "E0605");
    // A named group is not permission to combine targets that an ordinary
    // written list rejects. The module Policy/file PubFile mix still fails.
    rejects(&[("main.jet",
        "marker Defaults = #[Policy(copies: .Explicit), PubFile]\n#Defaults\nstruct Plain {}\nfn run() {}\n")], "E0355");
}
