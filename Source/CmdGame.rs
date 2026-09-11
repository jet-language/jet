//! `jet new game` project generator.
//!
//! The generated tree stays on the ordinary Package/source surfaces: the
//! command role files own the command entry points, `src/` owns typed source,
//! and `assets/` is the project asset root.  No hidden game-specific authority
//! is stored under `.jet/`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::exit;

use jet::ExitCodes;
use jet_foundation::Report::{StatusEnvelope, StatusValue};

use crate::OutputMode;


const GAME_NAME_TOKEN: &str = "__JET_GAME_NAME__";

const GAME_TEMPLATES: &[(&str, &str)] = &[
    ("package.jet", include_str!("../templates/game/package.jet")),
    ("@run.jet", include_str!("../templates/game/@run.jet")),
    ("@build.jet", include_str!("../templates/game/@build.jet")),
    ("@dev.jet", include_str!("../templates/game/@dev.jet")),
    ("@test.jet", include_str!("../templates/game/@test.jet")),
    ("src/Scene.jet", include_str!("../templates/game/src/Scene.jet")),
    (
        "src/Renderer.jet",
        include_str!("../templates/game/src/Renderer.jet"),
    ),
    (
        "src/Diagnostics.jet",
        include_str!("../templates/game/src/Diagnostics.jet"),
    ),
    (
        "src/Protocol.jet",
        include_str!("../templates/game/src/Protocol.jet"),
    ),
    (
        "src/Editor.jet",
        include_str!("../templates/game/src/Editor.jet"),
    ),
    (
        "src/Tooling.jet",
        include_str!("../templates/game/src/Tooling.jet"),
    ),
    (".gitignore", include_str!("../templates/game/.gitignore")),
    ("assets/.gitkeep", include_str!("../templates/game/assets/.gitkeep")),
];

/// `jet new game <name>` creates a source-backed, deterministic game package.
///
/// The caller owns command-line parsing and supplies the final project name;
/// this function deliberately has the same simple-folder contract as the
/// ordinary `jet new` path.  Generation is a transaction: every template is
/// validated and rendered in memory, written below a private staging
/// directory, and committed with one directory rename.
pub(crate) fn run_new_game(name: &str, mode: OutputMode) {
    if let Err(error) = validate_game_name(name) {
        report_failure(
            mode,
            "E2104",
            error,
            format!("try: {} new game my_game", jet::Syntax::BINARY_NAME),
        );
    }

    let templates = match render_game_templates(name) {
        Ok(templates) => templates,
        Err(error) => report_failure(
            mode,
            "E2105",
            format!("the packaged game templates are invalid: {error}"),
            "use the templates shipped with this Jet binary".to_string(),
        ),
    };

    let root = Path::new(name);
    if path_exists(root).unwrap_or_else(|error| {
        report_failure(
            mode,
            "E2105",
            format!("couldn't inspect `{}`: {error}", root.display()),
            "run `jet new game` from a readable directory, then retry".to_string(),
        )
    }) {
        report_failure(
            mode,
            "E2104",
            format!("`{name}` already exists"),
            "choose a new project name or move the existing project first".to_string(),
        );
    }

    let staging = staging_path(root, name);
    if path_exists(&staging).unwrap_or_else(|error| {
        report_failure(
            mode,
            "E2105",
            format!("couldn't inspect staging path `{}`: {error}", staging.display()),
            "remove the inaccessible staging path, then retry".to_string(),
        )
    }) {
        report_failure(
            mode,
            "E2104",
            format!("temporary staging path `{}` already exists", staging.display()),
            "remove that abandoned staging path only if it is not authored project data, then retry"
                .to_string(),
        );
    }

    if let Err(error) = fs::create_dir(&staging) {
        report_failure(
            mode,
            "E2105",
            format!("couldn't create staging directory `{}`: {error}", staging.display()),
            "run `jet new game` from a writable directory, then retry".to_string(),
        );
    }

    if let Err(error) = write_templates(&staging, &templates) {
        let error = cleanup_failure(&staging, error);
        report_failure(
            mode,
            "E2105",
            error,
            "run `jet new game` from a writable directory, then retry".to_string(),
        );
    }

    // Recheck immediately before commit.  This catches a target created while
    // templates were being written and prevents ordinary conflicts from being
    // replaced.  The final rename keeps the completed tree all-or-nothing.
    if path_exists(root).unwrap_or_else(|error| {
        let error = cleanup_failure(
            &staging,
            format!("couldn't inspect `{}` before commit: {error}", root.display()),
        );
        report_failure(
            mode,
            "E2105",
            error,
            "run `jet new game` from a writable directory, then retry".to_string(),
        )
    }) {
        let error = cleanup_failure(
            &staging,
            format!("`{name}` appeared while the game project was being created"),
        );
        report_failure(
            mode,
            "E2104",
            error,
            "preserved the existing target; choose another project name and retry".to_string(),
        );
    }

    if let Err(error) = fs::rename(&staging, root) {
        let error = cleanup_failure(
            &staging,
            format!("couldn't finalize game project `{name}`: {error}"),
        );
        report_failure(
            mode,
            "E2105",
            error,
            "run `jet new game` from a writable directory, then retry".to_string(),
        );
    }

    report_success(name, &templates, mode);
}

#[derive(Debug)]
struct RenderedTemplate {
    relative: &'static str,
    contents: String,
}

fn validate_game_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains(':')
        || name.contains('"')
        || name.contains(GAME_NAME_TOKEN)
        || name.ends_with('.')
        || name.ends_with(' ')
        || name.chars().any(char::is_control)
    {
        return Err("project name must be a simple folder name".to_string());
    }

    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err("project name must be a simple folder name".to_string());
    }

    Ok(())
}

fn render_game_templates(name: &str) -> Result<Vec<RenderedTemplate>, String> {
    if GAME_TEMPLATES.is_empty() {
        return Err("the game template table is empty".to_string());
    }

    let mut seen_paths = Vec::with_capacity(GAME_TEMPLATES.len());
    let mut saw_name_token = false;
    let mut rendered = Vec::with_capacity(GAME_TEMPLATES.len());

    for &(relative, source) in GAME_TEMPLATES {
        validate_template_path(relative)?;
        if seen_paths.iter().any(|path| *path == relative) {
            return Err(format!("duplicate game template path `{relative}`"));
        }
        seen_paths.push(relative);

        if source.contains('\0') {
            return Err(format!("game template `{relative}` contains a NUL byte"));
        }
        saw_name_token |= source.contains(GAME_NAME_TOKEN);

        let contents = source.replace(GAME_NAME_TOKEN, name);
        if contents.contains(GAME_NAME_TOKEN) {
            return Err(format!(
                "game template `{relative}` did not fully replace {GAME_NAME_TOKEN}"
            ));
        }
        rendered.push(RenderedTemplate { relative, contents });
    }

    if rendered.len() != GAME_TEMPLATES.len() {
        return Err("not every game template was prepared".to_string());
    }
    if !saw_name_token {
        return Err(format!("game templates do not contain {GAME_NAME_TOKEN}"));
    }

    Ok(rendered)
}

fn validate_template_path(relative: &str) -> Result<(), String> {
    if relative.is_empty()
        || relative.contains('\\')
        || relative.contains(GAME_NAME_TOKEN)
        || relative.chars().any(char::is_control)
        || Path::new(relative).is_absolute()
        || relative.split('/').any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(format!("invalid game template path `{relative}`"));
    }

    let mut components = Path::new(relative).components();
    while let Some(component) = components.next() {
        if !matches!(component, Component::Normal(_)) {
            return Err(format!("invalid game template path `{relative}`"));
        }
        if component.as_os_str() == ".jet" {
            return Err("game templates must not create hidden `.jet` state".to_string());
        }
    }

    Ok(())
}

fn staging_path(root: &Path, name: &str) -> PathBuf {
    let parent = root
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    parent.join(format!(".{name}.jet-new"))
}

fn path_exists(path: &Path) -> std::io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn write_templates(root: &Path, templates: &[RenderedTemplate]) -> Result<(), String> {
    for template in templates {
        let path = root.join(template.relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "couldn't create directory for `{}`: {error}",
                    template.relative
                )
            })?;
        }

        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| format!("couldn't write `{}`: {error}", template.relative))?;
        file.write_all(template.contents.as_bytes())
            .map_err(|error| format!("couldn't write `{}`: {error}", template.relative))?;
        file.sync_all()
            .map_err(|error| format!("couldn't sync `{}`: {error}", template.relative))?;
    }
    Ok(())
}

fn cleanup_failure(path: &Path, error: String) -> String {
    match fs::remove_dir_all(path) {
        Ok(()) => error,
        Err(cleanup) => format!(
            "{error}; couldn't remove staging directory `{}`: {cleanup}",
            path.display()
        ),
    }
}

fn report_success(name: &str, templates: &[RenderedTemplate], mode: OutputMode) {
    if mode.json {
        let files = StatusValue::array(
            templates
                .iter()
                .map(|template| StatusValue::String(template.relative.to_string())),
        );
        println!(
            "{}",
            StatusEnvelope::new("new_game", true)
                .with_field("name", name)
                .with_field("files", files)
                .json_line()
        );
        return;
    }

    if !mode.quiet {
        println!("created {name}/");
        for template in templates {
            println!("  {}", template.relative);
        }
        println!("next: cd {name} && {} run", jet::Syntax::BINARY_NAME);
    }
}

fn report_failure(mode: OutputMode, code: &str, what: String, fix: String) -> ! {
    let why = match code {
        "E2104" => "Jet needs valid command input before it can run this command",
        "E2105" => "Jet could not complete the named file, tool, or operating-system operation",
        _ => "Jet could not complete this command",
    };
    crate::emit_cli_report(code, what, why.to_string(), fix, mode.json);
    exit(ExitCodes::USER_ERROR);
}
