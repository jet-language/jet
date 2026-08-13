use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    write_yaml_std(&manifest);
    write_layout_rt(&manifest);
    write_reactive_rt(&manifest);
    write_regex_rt(&manifest);
    write_math_rt(&manifest);
    write_prelude_enum_meta(&manifest);
}

fn strip_rust_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            if i < bytes.len() {
                out.push('\n');
                i += 1;
            }
        } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                if bytes[i] == b'\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i = (i + 2).min(bytes.len());
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

fn rust_enum_variants(source: &str, rust_name: &str) -> Vec<String> {
    let source = strip_rust_comments(source);
    let needle = format!("enum {rust_name}");
    let mut search = 0;
    let start = loop {
        let Some(found) = source[search..].find(&needle) else {
            panic!("enum {rust_name} not found in Prelude source")
        };
        let found = search + found;
        let after = found + needle.len();
        let boundary = source[after..].chars().next();
        if !boundary.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
            break found;
        }
        search = after;
    };
    let open = source[start..]
        .find('{')
        .map(|offset| start + offset)
        .expect("Prelude enum opening brace");
    let bytes = source.as_bytes();
    let mut depth = 1usize;
    let mut segment_start = open + 1;
    let mut variants = Vec::new();
    let mut push_segment = |segment: &str| {
        let mut segment = segment.trim();
        while segment.starts_with("#[") {
            let Some(end) = segment.find(']') else {
                break;
            };
            segment = segment[end + 1..].trim();
        }
        let name: String = segment
            .chars()
            .skip_while(|ch| !ch.is_ascii_alphabetic() && *ch != '_')
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            .collect();
        if !name.is_empty() {
            variants.push(name);
        }
    };
    for i in open + 1..bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            b'}' if depth == 1 => {
                push_segment(&source[segment_start..i]);
                return variants;
            }
            b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 1 => {
                push_segment(&source[segment_start..i]);
                segment_start = i + 1;
            }
            _ => {}
        }
    }
    panic!("Prelude enum {rust_name} has no closing brace")
}

fn write_prelude_enum_meta(manifest: &PathBuf) {
    let specs = [
        ("TaskFailure", "../jet-codegen/src/Prelude/TaskGroup.rs", "JetTaskFailure"),
        ("ProcessStreamMode", "../jet-codegen/src/Prelude/CoreLib/JetStd/CommonTypes.rs", "ProcessStreamMode"),
        ("IOOperation", "../jet-codegen/src/Prelude/CoreLib/JetStd/Open.rs", "IOOperation"),
        ("IOError", "../jet-codegen/src/Prelude/CoreLib/JetStd/Open.rs", "IOError"),
        ("TextWidthAmbiguous", "../jet-codegen/src/Prelude/CoreLib/JetStd/Open.rs", "TextWidthAmbiguous"),
        ("TextWidthControls", "../jet-codegen/src/Prelude/CoreLib/JetStd/Open.rs", "TextWidthControls"),
        ("TerminalMode", "../jet-codegen/src/Prelude/CoreLib/JetStd/CommonTypes.rs", "TerminalMode"),
        ("Key", "../jet-codegen/src/Prelude/Core/RuntimeControl.rs", "JetKey"),
        ("EncodingFormat", "../jet-codegen/src/Prelude/CoreLib/JetStd/CommonTypes.rs", "EncodingFormat"),
        ("EncodingErrorKind", "../jet-codegen/src/Prelude/CoreLib/JetStd/CommonTypes.rs", "EncodingErrorKind"),
        ("DataEvent", "../jet-codegen/src/Prelude/CoreLib/JetStd/CommonTypes.rs", "DataEvent"),
        ("AuthError", "../jet-codegen/src/Prelude/CoreLib/Top/Auth.rs", "JetAuthError"),
        ("HookOutcome", "../jet-codegen/src/Prelude/CoreLib/JetStd/ReactiveEventWatch.rs", "JetHookOutcome"),
        ("HookDecision", "../jet-codegen/src/Prelude/CoreLib/JetStd/ReactiveEventWatch.rs", "JetHookDecision"),
        ("HookPolicy", "../jet-codegen/src/Prelude/CoreLib/JetStd/ReactiveEventWatch.rs", "JetHookPolicy"),
        ("Loadable", "../jet-codegen/src/Prelude/Core/Values.rs", "JetLoadable"),
        ("Overflow", "../jet-codegen/src/Prelude/CoreLib/JetStd/ReactiveEventWatch.rs", "JetEventOverflow"),
        ("FailurePolicy", "../jet-codegen/src/Prelude/CoreLib/JetStd/ReactiveEventWatch.rs", "JetFailurePolicy"),
        ("EventResult", "../jet-codegen/src/Prelude/Ui.rs", "JetEventResult"),
        ("DispatchState", "../jet-codegen/src/Prelude/CoreLib/JetStd/ReactiveEventWatch.rs", "JetDispatchState"),
        ("ServiceReceipt", "../jet-codegen/src/Prelude/CoreLib/Top/ServiceAuthority.rs", "JetServiceReceipt"),
        ("SMTPSecurity", "../jet-codegen/src/Prelude/CoreLib/Email.rs", "SMTPSecurity"),
        ("RecipientPolicy", "../jet-codegen/src/Prelude/CoreLib/Email.rs", "RecipientPolicy"),
        ("SMTPAuth", "../jet-codegen/src/Prelude/CoreLib/Email.rs", "SMTPAuth"),
        ("TLSTrust", "../jet-codegen/src/Prelude/CoreLib/Email.rs", "TLSTrust"),
        ("EmailError", "../jet-codegen/src/Prelude/CoreLib/Email.rs", "Error"),
        ("DataTree", "../jet-codegen/src/Prelude/CoreLib/JetStd/DataTree.rs", "DataTree"),
    ];
    let mut entries = Vec::new();
    for (jet_name, relative, rust_name) in specs {
        let path = manifest.join(relative);
        println!("cargo:rerun-if-changed={}", path.display());
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        entries.push((jet_name, rust_enum_variants(&source, rust_name)));
    }
    let data_tree = entries
        .iter()
        .find(|(name, _)| *name == "DataTree")
        .map(|(_, variants)| variants.clone())
        .expect("DataTree metadata");
    for alias in ["JSON", "TOML", "YAML", "CSV"] {
        entries.push((alias, data_tree.clone()));
    }

    let mut body = String::from("pub(crate) fn all() -> &'static [(&'static str, &'static [&'static str])] {\n    &[\n");
    for (name, variants) in &entries {
        body.push_str(&format!("        ({name:?}, &["));
        for variant in variants {
            body.push_str(&format!("{variant:?}, "));
        }
        body.push_str("]),\n");
    }
    body.push_str("    ]\n}\n");
    if let Some((_, variants)) = entries.iter().find(|(name, _)| *name == "DataTree") {
        for (index, variant) in variants.iter().enumerate() {
            body.push_str(&format!(
                "\npub(crate) const PRELUDE_DATATREE_{}: i64 = {index};\n",
                variant.to_ascii_uppercase()
            ));
        }
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("prelude_enum_meta.rs");
    std::fs::write(&out, body).expect("write prelude_enum_meta.rs");
}

fn write_math_rt(manifest: &PathBuf) {
    let src = manifest.join("../jet-codegen/src/Prelude/CoreLib/Top/MathLibPure.rs");
    println!("cargo:rerun-if-changed={}", src.display());
    let raw = std::fs::read_to_string(&src).expect("read MathLibPure.rs");
    // Pub crate-visible so CoreHost shims can call Prelude symbols by name.
    let body = raw
        .replace("\npub fn jet_std_math_", "\npub(crate) fn jet_std_math_")
        .replace("\nfn jet_std_math_", "\npub(crate) fn jet_std_math_");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("math_rt.rs");
    std::fs::write(&out, body).expect("write math_rt.rs");
}

fn write_reactive_rt(manifest: &PathBuf) {
    let src = manifest.join("../jet-codegen/src/Prelude/CoreLib/JetStd/ReactiveEventWatch.rs");
    println!("cargo:rerun-if-changed={}", src.display());
    let raw = std::fs::read_to_string(&src).expect("read ReactiveEventWatch.rs");
    // File is indented 4 spaces for jet_std string-concat embedding.
    let unindent = |s: &str| -> String {
        s.lines()
            .map(|line| line.strip_prefix("    ").unwrap_or(line))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let start = raw
        .find("// ── D-REACT1=B + D-DATARACE1=C")
        .expect("reactive marker");
    // Sync reactive + Event core, then skip AsyncEvent (needs task runtime),
    // then Hook/DecisionHook. JIT hosts async with thin adapters.
    let end_sync = raw
        .find("    pub struct JetAsyncPolicy")
        .expect("JetAsyncPolicy marker");
    let start_hooks = raw
        .find("    struct JetHookListener<")
        .expect("JetHookListener marker");
    let end_hooks = raw
        .find("    pub struct WatchHandle")
        .expect("WatchHandle marker");
    fn strip_orphan_derives(body: &mut String) {
        loop {
            let trimmed = body.trim_end();
            let orphan = trimmed.ends_with("#[derive(Clone)]")
                || trimmed.ends_with("#[derive(Clone, Copy)]")
                || trimmed.ends_with("#[derive(Clone, Copy, Debug, Eq, PartialEq)]")
                || trimmed.ends_with("#[derive(Clone, Copy, Debug, PartialEq, Eq)]");
            if !orphan {
                break;
            }
            if let Some(i) = body.rfind("#[derive") {
                body.truncate(i);
                *body = body.trim_end().to_string();
            } else {
                break;
            }
        }
    }
    let mut body = unindent(&raw[start..end_sync]);
    strip_orphan_derives(&mut body);
    body.push_str("\n");
    body.push_str(&unindent(&raw[start_hooks..end_hooks]));
    strip_orphan_derives(&mut body);
    // Pub types/fns JIT hosts need. Longest-first + already-pub skip so
    // `JetEvent` does not smash `JetEventScope` into `pub pub`.
    fn ensure_pub(body: &mut String, kind: &str, name: &str) {
        let bare = format!("{kind} {name}");
        let mut out = String::with_capacity(body.len() + 16);
        let bytes = body.as_str();
        let mut rest = bytes;
        while let Some(i) = rest.find(&bare) {
            let next = rest[i + bare.len()..].chars().next();
            let ident_continue = next.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
            out.push_str(&rest[..i]);
            if ident_continue {
                out.push_str(&bare);
            } else {
                let before = &rest[..i];
                let already = before.ends_with("pub ") || before.ends_with("pub(crate) ");
                if already {
                    out.push_str(&bare);
                } else {
                    out.push_str("pub ");
                    out.push_str(&bare);
                }
            }
            rest = &rest[i + bare.len()..];
        }
        out.push_str(rest);
        *body = out;
    }
    for name in [
        "JetReactiveEffect",
        "JetSignal",
        "JetDerived",
        "JetEventPolicy",
        "JetEventTrace",
        "JetEventScope",
        "JetEventOverflow",
        "JetEventConfigError",
        "JetSubscription",
        "JetEvent",
        "JetHookPolicy",
        "JetHookDecision",
        "JetHookOutcome",
        "JetHook",
        "JetDecisionHook",
        "JetAsyncEvent",
        "JetAsyncPolicy",
        "JetFailurePolicy",
        "JetDispatchReport",
        "JetDispatchState",
    ] {
        ensure_pub(&mut body, "struct", name);
        ensure_pub(&mut body, "enum", name);
    }
    for name in [
        "jet_reactive_effect_rooted",
        "jet_reactive_effect",
        "jet_reactive_scope",
    ] {
        ensure_pub(&mut body, "fn", name);
    }
    // Observe lives in Prelude::Observe; stub a no-op for the sync Event include.
    // AsyncEvent is host-shimmed (see Reactive.rs) — not included here.
    let stub = r#"
#[derive(Clone)]
pub struct JetObserveEvent {
    pub sequence: u64,
    pub source: &'static str,
    pub event_id: u64,
    pub owner_id: u64,
    pub subscription_id: u64,
    pub dispatch_id: u64,
    pub lifecycle: &'static str,
    pub queued: i64,
    pub blocked: i64,
    pub running: i64,
    pub capacity: i64,
    pub overflow: &'static str,
    pub priority: i64,
    pub failure: &'static str,
    pub terminal: &'static str,
}
pub fn jet_observe_event(_event: JetObserveEvent) {}
"#;
    body = body.replace("super::jet_observe_event", "jet_observe_event");
    body = body.replace("super::JetObserveEvent", "JetObserveEvent");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("reactive_rt.rs");
    std::fs::write(&out, format!("{stub}\n{body}\n")).expect("write reactive_rt.rs");
}

fn write_layout_rt(manifest: &PathBuf) {
    let src = manifest.join("../jet-codegen/src/Prelude/Layout.rs");
    println!("cargo:rerun-if-changed={}", src.display());
    let raw = std::fs::read_to_string(&src).expect("read Prelude/Layout.rs");
    let start = raw
        .find("mod jet_layout {")
        .expect("jet_layout module in Layout.rs");
    let body = &raw[start + "mod jet_layout {".len()..];
    let body = body.trim_end();
    let body = body
        .strip_suffix('}')
        .expect("Layout.rs closing brace")
        .trim_end();
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("layout_rt.rs");
    std::fs::write(&out, format!("{body}\n")).expect("write layout_rt.rs");
}

fn write_yaml_std(manifest: &PathBuf) {
    let src = manifest.join("../jet-codegen/src/Prelude/CoreLib/JetStd/YAML.rs");
    println!("cargo:rerun-if-changed={}", src.display());
    let raw = std::fs::read_to_string(&src).expect("read JetStd/YAML.rs");
    // YAML.rs ends with an extra `}` for corelib string-concat embedding.
    let trimmed = {
        let t = raw.trim_end();
        let without = t.strip_suffix('}').expect("YAML.rs trailing }");
        let without = without.trim_end();
        // Keep a trailing newline for include!
        format!("{without}\n")
    };
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("yaml_std.rs");
    std::fs::write(&out, trimmed).expect("write yaml_std.rs");
}

fn write_regex_rt(manifest: &PathBuf) {
    let src = manifest.join("../jet-codegen/src/Prelude/CoreLib/JetStd/Open.rs");
    println!("cargo:rerun-if-changed={}", src.display());
    let raw = std::fs::read_to_string(&src).expect("read Open.rs");
    let start = raw
        .find("    #[derive(Clone, Debug)]\n    pub struct RegexFlags {")
        .expect("RegexFlags derive in Open.rs");
    let mut body = raw[start..].to_string();
    while let Some(impl_at) = body.find("impl crate::JetShow for ") {
        let rest = &body[impl_at..];
        let close = rest
            .find("\n    }\n")
            .map(|i| i + 6)
            .expect("JetShow impl close");
        body.replace_range(impl_at..impl_at + close, "");
    }
    while let Some(impl_at) = body.find("impl crate::JetDebug for ") {
        let rest = &body[impl_at..];
        let close = rest
            .find("\n    }\n")
            .map(|i| i + 6)
            .expect("JetDebug impl close");
        body.replace_range(impl_at..impl_at + close, "");
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("regex_rt.rs");
    std::fs::write(&out, body).expect("write regex_rt.rs");
}
