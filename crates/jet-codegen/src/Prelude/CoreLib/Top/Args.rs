// ── D-ARGS1: declarative CLI arg parsing (ratified 2026-06-22) ───────────────
// The builder accumulates a spec; `jet_args_parse` runs it against an argv
// list, producing `ParsedArgs` or an error string. `jet_args_parse_or_exit`
// adds the conventional CLI boundary while keeping `jet_args_parse` pure.
//
// Design: builder methods take the spec BY VALUE and return a new one —
// ownership-safe, no aliasing, works with both immutable (::) and mutable
// (:=) bindings in Jet. The parse result is cloneable.
//
// `--help` is recognized but NOT parsed out of argv here; the caller tests
// `parsed.flag("help")` if they want to handle it. The auto-generated help
// text is available via `spec.help()` and `spec.help_auto()`.

/// A single entry in the spec.
#[derive(Clone)]
enum JetArgKind {
    /// Boolean flag: `--name` sets it to true.
    Flag {
        name: String,
        short: Option<String>,
        help: String,
    },
    /// Value option: `--name VALUE` captures VALUE.
    Option {
        name: String,
        short: Option<String>,
        help: String,
        meta: String,
        default: Option<String>,
        env: Option<String>,
        required: bool,
        repeat: bool,
        value: JetArgValueKind,
    },
    /// Positional argument (in declaration order).
    Positional { name: String, help: String },
    /// Subcommand with its own nested spec.
    Subcommand {
        name: String,
        help: String,
        spec: Box<JetArgsSpec>,
    },
}

#[derive(Clone)]
enum JetArgValueKind {
    String,
    Int,
    Float,
    Choice(Vec<String>),
}

/// The builder. All methods consume self and return a new spec (builder pattern).
#[derive(Clone)]
struct JetArgsSpec {
    entries: Vec<JetArgKind>,
    /// Root options inherited by a nested command. They parse at the command
    /// level but stay out of command help/completion so each shared flag has
    /// one visible declaration.
    inherited_entries: Vec<JetArgKind>,
    prog: String,
    description: Option<String>,
    version: Option<String>,
    inherited_version: bool,
}

/// The parse result.
#[derive(Clone)]
struct JetParsedArgs {
    flags: std::collections::HashMap<String, bool>,
    options: std::collections::HashMap<String, Vec<String>>,
    positionals: Vec<String>,
    subcommand: Option<String>,
    explicit_flags: std::collections::HashSet<String>,
    explicit_options: std::collections::HashSet<String>,
}

fn jet_args_program(mut spec: JetArgsSpec, prog: &str) -> JetArgsSpec {
    spec.prog = prog.to_string();
    spec
}

impl JetArgsSpec {
    /// Render the generated --help text.
    fn help(&self) -> String {
        let mut s = String::new();
        // usage line
        let prog = jet_args_source_program_name(&self.prog);
        let positionals: Vec<&JetArgKind> = self
            .entries
            .iter()
            .filter(|e| matches!(e, JetArgKind::Positional { .. }))
            .collect();
        s.push_str("Usage: ");
        s.push_str(&prog);
        for p in &positionals {
            if let JetArgKind::Positional { name, .. } = p {
                s.push(' ');
                s.push_str(name);
            }
        }
        // Every generated parser accepts `--help`, including a command with no
        // declared values of its own, so every usage line has options.
        s.push_str(" [options]");
        s.push('\n');
        if let Some(description) = &self.description {
            s.push('\n');
            s.push_str(description);
            s.push('\n');
        }
        // D-CLI-POS1: Arguments (positionals) before Options (flags).
        if !positionals.is_empty() {
            s.push('\n');
            s.push_str("Arguments:\n");
            for p in &positionals {
                if let JetArgKind::Positional { name, help } = p {
                    s.push_str(&format!("  {:<22} {}\n", name, help));
                }
            }
        }
        // flags and options
        let flags_opts: Vec<&JetArgKind> = self
            .entries
            .iter()
            .filter(|e| !matches!(e, JetArgKind::Positional { .. }))
            .collect();
        if !flags_opts.is_empty() {
            s.push('\n');
            s.push_str("Options:\n");
            for e in flags_opts {
                match e {
                    JetArgKind::Flag { name, short, help } => {
                        s.push_str(&format!("  {:<24} {}\n", jet_args_label(name, short, None), help));
                    }
                    JetArgKind::Option {
                        name,
                        short,
                        help,
                        meta,
                        default,
                        env,
                        required,
                        repeat,
                        value,
                    } => {
                        let mut note = help.clone();
                        if *required {
                            note.push_str(" (required)");
                        }
                        if *repeat {
                            note.push_str(" (repeatable)");
                        }
                        if let Some(d) = default {
                            note.push_str(&format!(" [default: {}]", d));
                        }
                        if let Some(e) = env {
                            note.push_str(&format!(" [env: {}]", e));
                        }
                        if let JetArgValueKind::Choice(choices) = value {
                            note.push_str(&format!(" [choices: {}]", choices.join(", ")));
                        }
                        s.push_str(&format!(
                            "  {:<24} {}\n",
                            jet_args_label(name, short, Some(meta)),
                            note
                        ));
                    }
                    JetArgKind::Subcommand { name, help, .. } => {
                        s.push_str(&format!("  {:<24} {}\n", name, help));
                    }
                    _ => {}
                }
            }
            s.push_str(&format!("  {:<24} {}\n", "--help", "show this help"));
            if self.version.is_some() {
                s.push_str(&format!("  {:<24} {}\n", "--version", "show version"));
            }
        }
        s
    }
}

fn jet_args_inherited_entries(spec: &JetArgsSpec) -> impl Iterator<Item = &JetArgKind> {
    spec.inherited_entries.iter()
}

fn jet_args_local_entries(spec: &JetArgsSpec) -> impl Iterator<Item = &JetArgKind> {
    spec.entries.iter()
}

fn jet_args_all_entries(spec: &JetArgsSpec) -> impl Iterator<Item = &JetArgKind> {
    jet_args_inherited_entries(spec)
        .chain(jet_args_local_entries(spec).filter(|entry| !matches!(entry, JetArgKind::Subcommand { .. })))
}

fn jet_args_same_input(left: &JetArgKind, right: &JetArgKind) -> bool {
    match (left, right) {
        (JetArgKind::Flag { name: left, .. }, JetArgKind::Flag { name: right, .. })
        | (JetArgKind::Option { name: left, .. }, JetArgKind::Option { name: right, .. })
        | (JetArgKind::Positional { name: left, .. }, JetArgKind::Positional { name: right, .. }) => left == right,
        _ => false,
    }
}

fn jet_args_label(name: &String, short: &Option<String>, meta: Option<&String>) -> String {
    let mut out = String::new();
    if let Some(s) = short {
        out.push('-');
        out.push_str(s);
        out.push_str(", ");
    }
    out.push_str("--");
    out.push_str(name);
    if let Some(m) = meta {
        out.push(' ');
        out.push_str(m);
    }
    out
}

fn jet_args_spec() -> JetArgsSpec {
    // argv[0] is the program name — capture it from env at spec-creation time.
    let prog = std::env::args().next().unwrap_or_default();
    JetArgsSpec {
        entries: Vec::new(),
        inherited_entries: Vec::new(),
        prog,
        description: None,
        version: None,
        inherited_version: false,
    }
}

fn jet_args_description(mut spec: JetArgsSpec, description: &String) -> JetArgsSpec {
    spec.description = Some(description.clone());
    spec
}

fn jet_args_flag(mut spec: JetArgsSpec, name: &String, help: &String) -> JetArgsSpec {
    spec.entries.push(JetArgKind::Flag {
        name: name.clone(),
        short: None,
        help: help.clone(),
    });
    spec
}

fn jet_args_flag_short(
    mut spec: JetArgsSpec,
    name: &String,
    short: &String,
    help: &String,
) -> JetArgsSpec {
    spec.entries.push(JetArgKind::Flag {
        name: name.clone(),
        short: Some(short.clone()),
        help: help.clone(),
    });
    spec
}

fn jet_args_option(
    mut spec: JetArgsSpec,
    name: &String,
    help: &String,
    meta: &String,
) -> JetArgsSpec {
    spec.entries.push(JetArgKind::Option {
        name: name.clone(),
        short: None,
        help: help.clone(),
        meta: meta.clone(),
        default: None,
        env: None,
        required: false,
        repeat: false,
        value: JetArgValueKind::String,
    });
    spec
}

fn jet_args_option_base(
    mut spec: JetArgsSpec,
    name: &String,
    short: Option<String>,
    help: &String,
    meta: &String,
    default: Option<String>,
    env: Option<String>,
    required: bool,
    repeat: bool,
    value: JetArgValueKind,
) -> JetArgsSpec {
    spec.entries.push(JetArgKind::Option {
        name: name.clone(),
        short,
        help: help.clone(),
        meta: meta.clone(),
        default,
        env,
        required,
        repeat,
        value,
    });
    spec
}

fn jet_args_option_short(
    spec: JetArgsSpec,
    name: &String,
    short: &String,
    help: &String,
    meta: &String,
) -> JetArgsSpec {
    jet_args_option_base(
        spec,
        name,
        Some(short.clone()),
        help,
        meta,
        None,
        None,
        false,
        false,
        JetArgValueKind::String,
    )
}

fn jet_args_option_default(
    spec: JetArgsSpec,
    name: &String,
    help: &String,
    meta: &String,
    default: &String,
) -> JetArgsSpec {
    jet_args_option_base(
        spec,
        name,
        None,
        help,
        meta,
        Some(default.clone()),
        None,
        false,
        false,
        JetArgValueKind::String,
    )
}

fn jet_args_option_env(
    spec: JetArgsSpec,
    name: &String,
    help: &String,
    meta: &String,
    env: &String,
) -> JetArgsSpec {
    jet_args_option_base(
        spec,
        name,
        None,
        help,
        meta,
        None,
        Some(env.clone()),
        false,
        false,
        JetArgValueKind::String,
    )
}

fn jet_args_option_int(
    spec: JetArgsSpec,
    name: &String,
    help: &String,
    meta: &String,
) -> JetArgsSpec {
    jet_args_option_base(
        spec,
        name,
        None,
        help,
        meta,
        None,
        None,
        false,
        false,
        JetArgValueKind::Int,
    )
}

fn jet_args_option_float(
    spec: JetArgsSpec,
    name: &String,
    help: &String,
    meta: &String,
) -> JetArgsSpec {
    jet_args_option_base(
        spec,
        name,
        None,
        help,
        meta,
        None,
        None,
        false,
        false,
        JetArgValueKind::Float,
    )
}

fn jet_args_option_choice(
    spec: JetArgsSpec,
    name: &String,
    help: &String,
    meta: &String,
    choices: &String,
) -> JetArgsSpec {
    let values = choices
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    jet_args_option_base(
        spec,
        name,
        None,
        help,
        meta,
        None,
        None,
        false,
        false,
        JetArgValueKind::Choice(values),
    )
}

fn jet_args_repeat(spec: JetArgsSpec, name: &String, help: &String, meta: &String) -> JetArgsSpec {
    jet_args_option_base(
        spec,
        name,
        None,
        help,
        meta,
        None,
        None,
        false,
        true,
        JetArgValueKind::String,
    )
}

fn jet_args_required_option(
    spec: JetArgsSpec,
    name: &String,
    help: &String,
    meta: &String,
) -> JetArgsSpec {
    jet_args_option_base(
        spec,
        name,
        None,
        help,
        meta,
        None,
        None,
        true,
        false,
        JetArgValueKind::String,
    )
}

fn jet_args_positional(mut spec: JetArgsSpec, name: &String, help: &String) -> JetArgsSpec {
    spec.entries.push(JetArgKind::Positional {
        name: name.clone(),
        help: help.clone(),
    });
    spec
}

fn jet_args_subcommand(
    mut spec: JetArgsSpec,
    name: &String,
    help: &String,
    mut sub: JetArgsSpec,
) -> JetArgsSpec {
    let mut inherited = spec.inherited_entries.clone();
    inherited.extend(
        spec.entries
            .iter()
            .filter(|entry| !matches!(entry, JetArgKind::Subcommand { .. }))
            .cloned(),
    );
    sub.entries.retain(|entry| {
        !inherited
            .iter()
            .any(|parent| jet_args_same_input(parent, entry))
    });
    for entry in inherited {
        if !sub
            .inherited_entries
            .iter()
            .any(|parent| jet_args_same_input(parent, &entry))
        {
            sub.inherited_entries.push(entry);
        }
    }
    sub.inherited_version |= spec.inherited_version || spec.version.is_some();
    if sub.inherited_version {
        sub.version = None;
    }
    spec.entries.push(JetArgKind::Subcommand {
        name: name.clone(),
        help: help.clone(),
        spec: Box::new(sub),
    });
    spec
}

fn jet_args_version(mut spec: JetArgsSpec, version: &String) -> JetArgsSpec {
    spec.version = Some(version.clone());
    spec
}

fn jet_args_completion(spec: &JetArgsSpec, shell: &String) -> String {
    let mut words = vec!["--help".to_string()];
    if spec.version.is_some() {
        words.push("--version".to_string());
    }
    for e in &spec.entries {
        match e {
            JetArgKind::Flag { name, short, .. }
            | JetArgKind::Option { name, short, .. } => {
                words.push(format!("--{}", name));
                if let Some(s) = short {
                    words.push(format!("-{}", s));
                }
            }
            JetArgKind::Subcommand { name, .. } => words.push(name.clone()),
            JetArgKind::Positional { name, .. } => words.push(name.clone()),
        }
    }
    format!("{} completion: {}", shell, words.join(" "))
}

/// Parse argv against the spec. Returns `Err(message)` on unknown flags/options
/// or missing required positionals. `argv[0]` (the program name) is skipped.
fn jet_args_parse(spec: &JetArgsSpec, argv: &Vec<String>) -> Result<JetParsedArgs, String> {
    let mut flags: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    let mut options: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    let mut fallbacks: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut positionals: Vec<String> = Vec::new();
    let mut subcommand: Option<String> = None;
    let mut explicit_flags: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut explicit_options: std::collections::HashSet<String> = std::collections::HashSet::new();
    let root_help = argv.len() == 1
        && spec
            .entries
            .iter()
            .any(|entry| matches!(entry, JetArgKind::Subcommand { .. }));

    // Seed all flags as false (so .flag("name") returns false when absent).
    flags.insert("help".to_string(), false);
    flags.insert("version".to_string(), false);
    for e in jet_args_all_entries(spec) {
        match e {
            JetArgKind::Flag { name, .. } => {
                flags.insert(name.clone(), false);
            }
            JetArgKind::Option { name, default, env, .. } => {
                // D-CLI-FIELD-MARKERS1=A: explicit argv is read below, then
                // environment, then the declared default.
                if let Some(v) = env
                    .as_ref()
                    .and_then(|key| std::env::var(key).ok())
                    .or_else(|| default.clone())
                {
                    fallbacks.insert(name.clone(), v);
                }
            }
            _ => {}
        }
    }

    let mut i = 1usize; // skip argv[0]
    while i < argv.len() {
        let arg = &argv[i];
        if arg == "--" {
            i += 1;
            // Everything after `--` is positional.
            while i < argv.len() {
                positionals.push(argv[i].clone());
                i += 1;
            }
            break;
        }
        if let Some(rest) = arg.strip_prefix("--") {
            if rest == "help" {
                flags.insert("help".to_string(), true);
                explicit_flags.insert("help".to_string());
                i += 1;
                continue;
            }
            if rest == "version" && (spec.version.is_some() || spec.inherited_version) {
                flags.insert("version".to_string(), true);
                explicit_flags.insert("version".to_string());
                i += 1;
                continue;
            }
            // Try `--name=value` form.
            if let Some(eq) = rest.find('=') {
                let name = &rest[..eq];
                let val = &rest[eq + 1..];
                if let Some(entry) = jet_args_find_option(spec, name) {
                    jet_args_store_option(&mut options, entry, val)?;
                    if let JetArgKind::Option { name, .. } = entry {
                        explicit_options.insert(name.clone());
                    }
                } else if jet_args_find_flag(spec, name).is_some() {
                    return Err(format!(
                        "--{} is a flag; it takes no value (got `={}`)\n\n{}",
                        name,
                        val,
                        spec.help()
                    ));
                } else {
                    return Err(jet_args_unknown(name, spec));
                }
            } else if jet_args_find_flag(spec, rest).is_some() {
                flags.insert(rest.to_string(), true);
                explicit_flags.insert(rest.to_string());
            } else if let Some(entry) = jet_args_find_option(spec, rest) {
                i += 1;
                if i >= argv.len() {
                    return Err(format!("`--{}` requires a value\n\n{}", rest, spec.help()));
                }
                jet_args_store_option(&mut options, entry, &argv[i])?;
                if let JetArgKind::Option { name, .. } = entry {
                    explicit_options.insert(name.clone());
                }
            } else {
                return Err(jet_args_unknown(rest, spec));
            }
        } else if arg.starts_with('-') && arg.len() > 1 {
            let rest = &arg[1..];
            if rest.len() > 1 {
                let mut chars = rest.chars().peekable();
                while let Some(ch) = chars.next() {
                    let short = ch.to_string();
                    if let Some((name, is_option)) = jet_args_find_short(spec, &short) {
                        if is_option {
                            let value: String = chars.collect();
                            if value.is_empty() {
                                i += 1;
                                if i >= argv.len() {
                                    return Err(format!("`-{}` requires a value\n\n{}", short, spec.help()));
                                }
                                let entry = jet_args_find_option(spec, &name).unwrap();
                                jet_args_store_option(&mut options, entry, &argv[i])?;
                                explicit_options.insert(name.clone());
                            } else {
                                let entry = jet_args_find_option(spec, &name).unwrap();
                                jet_args_store_option(&mut options, entry, &value)?;
                                explicit_options.insert(name.clone());
                            }
                            break;
                        }
                        explicit_flags.insert(name.clone());
                        flags.insert(name, true);
                    } else {
                        return Err(format!("unknown option `-{}`\n\n{}", short, spec.help()));
                    }
                }
            } else if let Some((name, is_option)) = jet_args_find_short(spec, rest) {
                if is_option {
                    i += 1;
                    if i >= argv.len() {
                        return Err(format!("`-{}` requires a value\n\n{}", rest, spec.help()));
                    }
                    let entry = jet_args_find_option(spec, &name).unwrap();
                    jet_args_store_option(&mut options, entry, &argv[i])?;
                    explicit_options.insert(name.clone());
                } else {
                    explicit_flags.insert(name.clone());
                    flags.insert(name, true);
                }
            } else {
                return Err(format!("unknown option `-{}`\n\n{}", rest, spec.help()));
            }
        } else {
            if subcommand.is_none() {
                if let Some((name, nested)) = jet_args_find_subcommand(spec, arg) {
                    let mut nested_argv = vec![format!("{} {}", spec.prog, name)];
                    nested_argv.extend(argv.iter().skip(i + 1).cloned());
                    let parsed = jet_args_parse(&nested, &nested_argv)?;
                    let nested_explicit_flags = parsed.explicit_flags;
                    let nested_explicit_options = parsed.explicit_options;
                    for (name, value) in parsed.flags {
                        if nested_explicit_flags.contains(&name) || !flags.contains_key(&name) {
                            flags.insert(name, value);
                        }
                    }
                    for (name, value) in parsed.options {
                        if nested_explicit_options.contains(&name) || !options.contains_key(&name) {
                            options.insert(name, value);
                        }
                    }
                    positionals.extend(parsed.positionals);
                    explicit_flags.extend(nested_explicit_flags);
                    explicit_options.extend(nested_explicit_options);
                    subcommand = Some(name.to_string());
                    break;
                }
                if spec
                    .entries
                    .iter()
                    .any(|entry| matches!(entry, JetArgKind::Subcommand { .. }))
                {
                    return Err(format!(
                        "unknown command `{}`\n\n{}",
                        arg,
                        spec.help()
                    ));
                }
            }
            positionals.push(arg.clone());
        }
        i += 1;
    }

    // D-CLI-POS1=A named-wins: a positional whose name matches an already-set
    // option is satisfied by the named form and does not consume a bare arg.
    // Remaining bare args fill unsatisfied positionals in declaration order
    // by copying into `options` under the positional name so decode can read
    // one path (`jet_parsed_option`) for both forms.
    let mut bare_i = 0usize;
    let mut missing: Vec<&str> = Vec::new();
    for e in jet_args_all_entries(spec) {
        let JetArgKind::Positional { name, .. } = e else {
            continue;
        };
        if options.contains_key(name) {
            continue;
        }
        if bare_i < positionals.len() {
            let value = &positionals[bare_i];
            // Prefer typed store when a same-named option exists (derive pairing).
            if let Some(entry) = jet_args_find_option(spec, name) {
                jet_args_store_option(&mut options, entry, value)?;
            } else {
                options.insert(name.clone(), vec![value.clone()]);
            }
            bare_i += 1;
        } else if let Some(value) = fallbacks.remove(name) {
            options.insert(name.clone(), vec![value]);
        } else {
            missing.push(name.as_str());
        }
    }
    for (name, value) in fallbacks {
        options.entry(name).or_insert_with(|| vec![value]);
    }
    if !missing.is_empty()
        && !root_help
        && !flags.get("help").copied().unwrap_or(false)
    {
        return Err(format!(
            "missing required argument{}: {}\n\n{}",
            if missing.len() == 1 { "" } else { "s" },
            missing.join(", "),
            spec.help()
        ));
    }
    for e in jet_args_all_entries(spec) {
        if let JetArgKind::Option { name, required, .. } = e {
            if *required
                && !options.contains_key(name)
                && !root_help
                && !flags.get("help").copied().unwrap_or(false)
            {
                return Err(format!("missing required option `--{}`\n\n{}", name, spec.help()));
            }
        }
    }

    Ok(JetParsedArgs {
        flags,
        options,
        positionals,
        subcommand,
        explicit_flags,
        explicit_options,
    })
}

fn jet_args_parse_or_exit(spec: &JetArgsSpec, argv: &Vec<String>) -> JetParsedArgs {
    match jet_args_parse(spec, argv) {
        Ok(parsed) if jet_parsed_flag(&parsed, &"help".to_string()) => {
            println!("{}", spec.help());
            std::process::exit(0);
        }
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("{}", message);
            std::process::exit(2);
        }
    }
}

fn jet_args_find_flag<'a>(spec: &'a JetArgsSpec, name: &str) -> Option<&'a JetArgKind> {
    jet_args_all_entries(spec)
        .find(|e| matches!(e, JetArgKind::Flag { name: n, .. } if n == name))
}

fn jet_args_find_option<'a>(spec: &'a JetArgsSpec, name: &str) -> Option<&'a JetArgKind> {
    jet_args_all_entries(spec)
        .find(|e| matches!(e, JetArgKind::Option { name: n, .. } if n == name))
}

fn jet_args_find_short(spec: &JetArgsSpec, short: &str) -> Option<(String, bool)> {
    for e in jet_args_all_entries(spec) {
        match e {
            JetArgKind::Flag { name, short: Some(s), .. } if s == short => {
                return Some((name.clone(), false));
            }
            JetArgKind::Option { name, short: Some(s), .. } if s == short => {
                return Some((name.clone(), true));
            }
            _ => {}
        }
    }
    None
}

fn jet_args_find_subcommand<'a>(spec: &'a JetArgsSpec, name: &str) -> Option<(&'a str, &'a JetArgsSpec)> {
    spec.entries.iter().find_map(|e| {
        if let JetArgKind::Subcommand { name: n, spec, .. } = e {
            (n == name).then_some((n.as_str(), spec.as_ref()))
        } else {
            None
        }
    })
}

fn jet_args_store_option(
    options: &mut std::collections::HashMap<String, Vec<String>>,
    entry: &JetArgKind,
    value: &str,
) -> Result<(), String> {
    if let JetArgKind::Option { name, repeat, value: kind, .. } = entry {
        match kind {
            JetArgValueKind::String => {}
            JetArgValueKind::Int => {
                value.parse::<i64>().map_err(|_| format!(
                    "`--{}` expects an Int, got `{}`\n\nfix: pass a whole number to `--{}`",
                    name, value, name
                ))?;
            }
            JetArgValueKind::Float => {
                value.parse::<f64>().map_err(|_| format!(
                    "`--{}` expects a Float, got `{}`\n\nfix: pass a number to `--{}`",
                    name, value, name
                ))?;
            }
            JetArgValueKind::Choice(choices) => {
                if !choices.iter().any(|c| c == value) {
                    return Err(format!(
                        "`--{}` expects one of: {}; got `{}`",
                        name,
                        choices.join(", "),
                        value
                    ));
                }
            }
        }
        if *repeat {
            options.entry(name.clone()).or_default().push(value.to_string());
        } else {
            options.insert(name.clone(), vec![value.to_string()]);
        }
    }
    Ok(())
}

fn jet_args_unknown(name: &str, spec: &JetArgsSpec) -> String {
    // No typed edit: this parser validates runtime argv, not Jet source, so it
    // has no source file/span for `jet.report/v1` to project into `fix_edits`.
    let known: Vec<String> = jet_args_all_entries(spec).filter_map(|e| match e {
        JetArgKind::Flag { name, .. } | JetArgKind::Option { name, .. } => Some(name.clone()),
        _ => None,
    }).collect();
    let suggestion = known
        .iter()
        .find(|k| jet_args_edit_distance(k, name) <= 2)
        .map(|k| format!("\ndid you mean `--{}`?", k))
        .unwrap_or_default();
    format!("unknown option `--{}`{}\n\n{}", name, suggestion, spec.help())
}

fn jet_args_edit_distance(a: &str, b: &str) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.chars().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            cur[j + 1] = if ca == cb {
                prev[j]
            } else {
                1 + prev[j].min(prev[j + 1]).min(cur[j])
            };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

fn jet_parsed_flag(parsed: &JetParsedArgs, name: &String) -> bool {
    *parsed.flags.get(name.as_str()).unwrap_or(&false)
}

fn jet_parsed_option(parsed: &JetParsedArgs, name: &String) -> JetOutcome<String, JetAbsent> {
    jet_outcome_of(parsed.options.get(name.as_str()).and_then(|v| v.last().cloned()))
}

fn jet_parsed_option_int(parsed: &JetParsedArgs, name: &String) -> JetOutcome<i64, JetAbsent> {
    jet_outcome_of(jet_parsed_option(parsed, name).ok().and_then(|v| v.parse::<i64>().ok()))
}

fn jet_parsed_option_float(parsed: &JetParsedArgs, name: &String) -> JetOutcome<f64, JetAbsent> {
    jet_outcome_of(jet_parsed_option(parsed, name).ok().and_then(|v| v.parse::<f64>().ok()))
}

fn jet_parsed_options(parsed: &JetParsedArgs, name: &String) -> Vec<String> {
    parsed.options.get(name.as_str()).cloned().unwrap_or_default()
}

fn jet_parsed_positional(parsed: &JetParsedArgs, idx: i64) -> JetOutcome<String, JetAbsent> {
    if idx < 0 {
        return Err(JetAbsent);
    }
    jet_outcome_of(parsed.positionals.get(idx as usize).cloned())
}

fn jet_parsed_subcommand(parsed: &JetParsedArgs) -> JetOutcome<String, JetAbsent> {
    jet_outcome_of(parsed.subcommand.clone())
}

/// D-CLI-GLOBAL1=E: the Standard pack maps verbosity onto the existing
/// `core.log` levels. `--quiet` wins when both switches are present so a
/// script can silence a verbose default explicitly.
fn jet_args_standard_log_level(parsed: &JetParsedArgs) -> String {
    if jet_parsed_flag(parsed, &"quiet".to_string()) {
        "error".to_string()
    } else if jet_parsed_flag(parsed, &"verbose".to_string()) {
        "debug".to_string()
    } else {
        "info".to_string()
    }
}

/// D-CLI-GLOBAL1=E: normalize the Standard color choice once. `auto` follows
/// `NO_COLOR`; explicit `always` and `never` are expert overrides.
fn jet_args_standard_color_mode(parsed: &JetParsedArgs) -> String {
    let requested = jet_parsed_option(parsed, &"color".to_string())
        .ok()
        .unwrap_or_else(|| "auto".to_string());
    match requested.as_str() {
        "always" => "always".to_string(),
        "never" => "never".to_string(),
        _ if std::env::var_os("NO_COLOR").is_some() => "never".to_string(),
        _ => "auto".to_string(),
    }
}

impl JetShow for JetArgsSpec {
    fn jet_show(&self) -> String {
        format!("ArgsSpec({})", self.entries.len())
    }
}
impl JetShow for JetParsedArgs {
    fn jet_show(&self) -> String {
        format!(
            "ParsedArgs(flags={}, options={}, positionals={})",
            self.flags.len(),
            self.options.len(),
            self.positionals.len()
        )
    }
}
