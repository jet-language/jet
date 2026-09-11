// D-SHAPE-PROJECT1: root-level semantic adapters for typed argument
// projection. This file is included once, after the canonical Shape source and
// raw Args Prelude source, so multiply-included host fragments stay
// dependency-minimal.

/// D-SHAPE-ONE1=A: decode a typed value anywhere through the same builder
/// used by `fn run(args: T)`. The parser remains the only argv reader; this
/// adapter only marshals the shared host-neutral tree into the canonical
/// `DataTree` consumed by `__jet_Decode`.
fn jet_args_decode<T: __jet_Decode>(
    spec: &JetArgsSpec,
    argv: &Vec<String>,
    names: &[(&str, &str)],
) -> Result<T, Vec<jet_std::FieldError>> {
    let parsed = jet_args_parse(spec, argv).map_err(|message| jet_std::FieldError::one(message))?;
    let fields = jet_args_shape_tree_values(spec, &parsed, names).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| {
                if error.path.is_empty() {
                    jet_std::FieldError::one(error.reason)
                } else {
                    jet_std::FieldError::at(error.path, error.reason)
                }
            })
            .collect::<Vec<_>>()
    })?;
    let fields = fields
        .into_iter()
        .map(|(name, value)| (name, jet_args_shape_value_to_datatree(value)))
        .collect();
    T::jet_decode(&jet_std::DataTree::Object(fields))
}

fn jet_args_shape_value_to_datatree(value: JetArgsShapeValue) -> jet_std::DataTree {
    match value {
        JetArgsShapeValue::Bool(value) => jet_std::DataTree::Bool(value),
        JetArgsShapeValue::Int(value) => jet_std::DataTree::Int(value),
        JetArgsShapeValue::Float(value) => jet_std::DataTree::Float(value),
        JetArgsShapeValue::Text(value) => jet_std::DataTree::Text(value),
        JetArgsShapeValue::Array(values) => jet_std::DataTree::Array(
            values
                .into_iter()
                .map(jet_args_shape_value_to_datatree)
                .collect(),
        ),
    }
}

/// D-SHAPE-PROJECT1=A: `T.merge(flags, settings)`. Presence comes from the
/// same builder parse `args.decode<T>()` ran: a field named on argv keeps the
/// flag layer's value, every other field takes the settings layer, whose own
/// decode already applied the environment value or the field default. An
/// invalid present value never reaches this function: each layer's decoder
/// returned `FieldError` for it.
fn jet_config_merge<T: __jet_Encode + __jet_Decode>(
    spec: &JetArgsSpec,
    argv: &Vec<String>,
    names: &[(&str, &str)],
    flags: &T,
    settings: &T,
) -> Result<T, Vec<jet_std::FieldError>> {
    let jet_std::DataTree::Object(flag_fields) = flags.jet_encode() else {
        return Err(jet_std::FieldError::one("flags layer is not a record"));
    };
    let jet_std::DataTree::Object(settings_fields) = settings.jet_encode() else {
        return Err(jet_std::FieldError::one("settings layer is not a record"));
    };
    let fields = jet_args_merge_fields(spec, argv, names, flag_fields, settings_fields)
        .map_err(|errors| {
            errors
                .into_iter()
                .map(|error| {
                    if error.path.is_empty() {
                        jet_std::FieldError::one(error.reason)
                    } else {
                        jet_std::FieldError::at(error.path, error.reason)
                    }
                })
                .collect::<Vec<_>>()
        })?;
    T::jet_decode(&jet_std::DataTree::Object(fields))
}

#[derive(Clone)]
struct JetGuidedTuiModel {
    label: String,
    value: String,
    cursor: usize,
    width: usize,
    error: Option<String>,
    done: bool,
}

enum JetGuidedTuiMessage {
    Key { code: String, modifiers: u8 },
    Resize(JetSize),
    Reset {
        value: String,
        error: Option<String>,
    },
}

fn jet_guided_tui_update(
    mut model: JetGuidedTuiModel,
    message: JetGuidedTuiMessage,
) -> (JetGuidedTuiModel, JetTuiCommand<JetGuidedTuiMessage>) {
    match message {
        JetGuidedTuiMessage::Reset { value, error } => {
            model.value = value;
            model.cursor = model.value.chars().count();
            model.error = error;
            model.done = false;
        }
        JetGuidedTuiMessage::Resize(size) => {
            model.width = size.width.max(1.0).floor() as usize;
        }
        JetGuidedTuiMessage::Key { code, modifiers } => {
            if modifiers & 1 != 0 {
                match code.as_str() {
                    "c" => {
                        model.error = Some("guided input cancelled".to_string());
                        model.done = true;
                    }
                    "u" => {
                        model.value.clear();
                        model.cursor = 0;
                    }
                    _ => {}
                }
            } else {
                match code.as_str() {
                    "enter" => model.done = true,
                    "escape" => {
                        model.error = Some("guided input cancelled".to_string());
                        model.done = true;
                    }
                    "left" => model.cursor = model.cursor.saturating_sub(1),
                    "right" => {
                        model.cursor = (model.cursor + 1).min(model.value.chars().count());
                    }
                    "backspace" => {
                        if model.cursor > 0 {
                            let mut chars = model.value.chars().collect::<Vec<_>>();
                            chars.remove(model.cursor - 1);
                            model.value = chars.into_iter().collect();
                            model.cursor -= 1;
                        }
                    }
                    "delete" => {
                        let mut chars = model.value.chars().collect::<Vec<_>>();
                        if model.cursor < chars.len() {
                            chars.remove(model.cursor);
                            model.value = chars.into_iter().collect();
                        }
                    }
                    "up" | "down" | "tab" | "unknown" => {}
                    _ if !code.is_empty() => {
                        let mut chars = model.value.chars().collect::<Vec<_>>();
                        let inserted = code.chars().collect::<Vec<_>>();
                        chars.splice(model.cursor..model.cursor, inserted.iter().copied());
                        model.value = chars.into_iter().collect();
                        model.cursor += inserted.len();
                    }
                    _ => {}
                }
            }
            if !model.done && !matches!(code.as_str(), "enter" | "escape") {
                model.error = None;
            }
        }
    }
    (model, JetTuiCommand::none())
}

fn jet_guided_visible_input(model: &JetGuidedTuiModel) -> String {
    let chars = model.value.chars().collect::<Vec<_>>();
    let cursor = model.cursor.min(chars.len());
    let width = model.width.max(8).saturating_sub(1);
    let start = cursor
        .saturating_sub(width.saturating_sub(1))
        .min(chars.len().saturating_sub(width));
    let end = (start + width).min(chars.len());
    let mut visible: String = chars[start..end].iter().copied().collect();
    let caret = cursor.saturating_sub(start);
    let caret_byte = visible
        .char_indices()
        .nth(caret)
        .map_or(visible.len(), |(index, _)| index);
    visible.insert(caret_byte, '▏');
    visible
}

fn jet_guided_tui_view(model: &JetGuidedTuiModel) -> JetUiNode {
    let mut children = vec![
        jet_ui_text(&model.label),
        jet_ui_text_input(&jet_guided_visible_input(model), JetUiImeMode::Native),
    ];
    if let Some(error) = &model.error {
        children.push(jet_ui_text(&format!("Correction: {error}")));
    }
    jet_ui_box(children)
}

fn jet_guided_tui_frame(
    program: &JetTuiProgram<
        JetGuidedTuiModel,
        JetGuidedTuiMessage,
        fn(
            JetGuidedTuiModel,
            JetGuidedTuiMessage,
        ) -> (JetGuidedTuiModel, JetTuiCommand<JetGuidedTuiMessage>),
        fn(&JetGuidedTuiModel) -> JetUiNode,
    >,
) -> Result<(), String> {
    let frame = program.frame_lines().join(" | ");
    jet_term_write_stderr(&format!("\r\x1b[2K{frame}"), true)
        .map_err(|error| format!("guided prompt output failed: {error}"))
}

fn jet_args_guided_tui_prompt(
    field: &JetGuidedField,
    initial: &str,
    error: Option<&str>,
) -> Result<String, String> {
    let label = if field.help.is_empty() {
        format!("Page {} · {}:", field.page + 1, field.name)
    } else {
        format!("Page {} · {} — {}:", field.page + 1, field.name, field.help)
    };
    let width = jet_term_width(|name| std::env::var(name).ok()).max(1) as usize;
    let model = JetGuidedTuiModel {
        label,
        value: initial.to_string(),
        cursor: initial.chars().count(),
        width,
        error: error.map(str::to_string),
        done: false,
    };
    let mut program = JetTuiProgram::new(model, jet_guided_tui_update, jet_guided_tui_view);
    program.set_constraint(jet_ui_constraint(
        0.0,
        0.0,
        jet_term_width(|name| std::env::var(name).ok()) as f64,
        jet_term_height(|name| std::env::var(name).ok()).min(8) as f64,
    ));
    program.set_event_mapper(|event| match event {
        JetTuiEvent::Key { code, modifiers } => {
            Some(JetGuidedTuiMessage::Key { code, modifiers })
        }
        JetTuiEvent::Resize { size } => Some(JetGuidedTuiMessage::Resize(size)),
        JetTuiEvent::Interrupt | JetTuiEvent::Close => Some(JetGuidedTuiMessage::Key {
            code: "escape".to_string(),
            modifiers: 0,
        }),
        JetTuiEvent::Timer { .. }
        | JetTuiEvent::Io { .. }
        | JetTuiEvent::Focus { .. } => None,
    });
    program.redraw();
    jet_args_guided_tui_frame(&program)?;
    jet_term_enter();
    let _resize = program.dispatch_event(JetTuiEvent::resize(
        width as f64,
        jet_term_height(|name| std::env::var(name).ok()) as f64,
    ));
    program.step();
    loop {
        let key = jet_term_read_key();
        if matches!(key, JetKey::Unknown) {
            jet_term_leave();
            return Err("guided input ended before the form was submitted".to_string());
        }
        let event = JetTuiEvent::key_with_modifiers(&key.code(), key.modifier_bits());
        program.dispatch_event(event);
        program.step();
        if let Err(error) = jet_args_guided_tui_frame(&program) {
            jet_term_leave();
            return Err(error);
        }
        let Some(model) = program.model() else {
            jet_term_leave();
            return Err("guided input model stopped unexpectedly".to_string());
        };
        if model.done {
            let result = if let Some(error) = model.error.clone() {
                Err(error)
            } else {
                Ok(model.value.clone())
            };
            jet_term_leave();
            jet_term_write_stderr("\n", true)
                .map_err(|output_error| format!("guided prompt output failed: {output_error}"))?;
            return result;
        }
    }
}

fn jet_args_guided_line_prompt(
    field: &JetGuidedField,
    initial: &str,
    error: Option<&str>,
) -> Result<String, String> {
    let mut prompt = if field.help.is_empty() {
        format!("{}:", field.name)
    } else {
        format!("{} — {}:", field.name, field.help)
    };
    if !initial.is_empty() {
        prompt.push_str(&format!(" [{}]", initial));
    }
    if let Some(error) = error {
        prompt.push_str(&format!(" Correction: {error}"));
    }
    prompt.push(' ');
    jet_term_write_stderr(&prompt, true)
        .map_err(|output_error| format!("guided prompt output failed: {output_error}"))?;
    match jet_term_read_stdin_line() {
        JetTermRead::Line(value) => Ok(value),
        JetTermRead::EndOfInput => {
            Err("guided input ended before the form was submitted".to_string())
        }
    }
}

fn jet_args_guided_interactive() -> bool {
    jet_term_stdin_is_terminal()
        && jet_term_stderr_is_terminal()
        && !jet_term_machine_output()
        && std::env::var_os("CI").is_none()
}

fn jet_args_guided_accessible() -> bool {
    std::env::var_os("NO_COLOR").is_some()
        || std::env::var_os("JET_ACCESSIBLE").is_some()
        || std::env::var_os("JET_CLI_ACCESSIBLE").is_some()
        || std::env::var("TERM")
            .ok()
            .is_some_and(|term| term.eq_ignore_ascii_case("dumb"))
}

fn jet_args_guided_argv_tui(
    spec: &JetArgsSpec,
    argv: &Vec<String>,
) -> Result<Vec<String>, String> {
    if !jet_args_guided_interactive() {
        return Ok(argv.clone());
    }
    if jet_args_guided_accessible() {
        jet_args_guided_argv_with(spec, argv, jet_args_guided_line_prompt)
    } else {
        jet_args_guided_argv_with(spec, argv, jet_args_guided_tui_prompt)
    }
}
