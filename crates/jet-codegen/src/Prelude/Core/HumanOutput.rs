use std::convert::TryFrom;

// D-DX-HUMANOUTPUT1: one capability-aware human output kernel.  The machine
// channel never consumes these renderables; callers choose the explicit
// structured serializer instead.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum JetOutputColorMode {
    #[default]
    Auto,
    Always,
    Never,
}

impl JetOutputColorMode {
    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "always" => Self::Always,
            "never" => Self::Never,
            _ => Self::Auto,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Always => "always",
            Self::Never => "never",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JetOutputChannel {
    HumanStdout,
    HumanStderr,
    MachineStdout,
    Hidden,
}

impl JetOutputChannel {
    pub(crate) const fn is_human(self) -> bool {
        matches!(self, Self::HumanStdout | Self::HumanStderr)
    }

    pub(crate) const fn is_machine(self) -> bool {
        matches!(self, Self::MachineStdout)
    }

    pub(crate) const fn is_hidden(self) -> bool {
        matches!(self, Self::Hidden)
    }

    pub(crate) const fn is_stdout(self) -> bool {
        matches!(self, Self::HumanStdout | Self::MachineStdout)
    }

    pub(crate) const fn is_stderr(self) -> bool {
        matches!(self, Self::HumanStderr)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JetOutputKind {
    Renderable,
    Status,
    Log,
    Progress,
    Machine,
}

impl JetOutputKind {
    pub(crate) const fn is_machine(self) -> bool {
        matches!(self, Self::Machine)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct JetOutputCapabilities {
    pub(crate) channel: JetOutputChannel,
    pub(crate) requested_color: JetOutputColorMode,
    pub(crate) stdin_is_terminal: bool,
    pub(crate) stdout_is_terminal: bool,
    pub(crate) stderr_is_terminal: bool,
    pub(crate) term_is_dumb: bool,
    pub(crate) no_color: bool,
    pub(crate) force_color: bool,
    pub(crate) json: bool,
    pub(crate) color_enabled: bool,
    pub(crate) unicode_enabled: bool,
    pub(crate) progress_enabled: bool,
    pub(crate) width: usize,
    pub(crate) height: usize,
}

impl JetOutputCapabilities {
    pub(crate) const fn human(self) -> bool {
        self.channel.is_human()
    }

    pub(crate) const fn machine(self) -> bool {
        self.channel.is_machine()
    }

    pub(crate) const fn human_enabled(self) -> bool {
        self.human()
    }

    pub(crate) const fn machine_enabled(self) -> bool {
        self.machine()
    }

    pub(crate) const fn ansi_enabled(self) -> bool {
        self.color_enabled
    }

    pub(crate) const fn unicode_enabled(self) -> bool {
        self.unicode_enabled
    }

    pub(crate) const fn progress_enabled(self) -> bool {
        self.progress_enabled
    }

    pub(crate) const fn channel(self, kind: JetOutputKind) -> JetOutputChannel {
        self.channel_for(kind)
    }

    pub(crate) const fn channel_for(self, kind: JetOutputKind) -> JetOutputChannel {
        if self.channel.is_machine() {
            return if kind.is_machine() {
                JetOutputChannel::MachineStdout
            } else {
                JetOutputChannel::Hidden
            };
        }
        if self.channel.is_hidden() {
            return JetOutputChannel::Hidden;
        }
        match kind {
            JetOutputKind::Renderable => JetOutputChannel::HumanStdout,
            JetOutputKind::Status | JetOutputKind::Log | JetOutputKind::Progress => {
                JetOutputChannel::HumanStderr
            }
            JetOutputKind::Machine => JetOutputChannel::Hidden,
        }
    }

    pub(crate) const fn allows(self, kind: JetOutputKind) -> bool {
        !self.channel_for(kind).is_hidden()
    }
}

/// Build a capability profile from facts supplied by a host or test. Width
/// and height are clamped because a zero terminal dimension cannot lay out a
/// row and should not turn into an unbounded fallback.
#[allow(clippy::too_many_arguments)]
pub(crate) fn jet_output_capabilities(
    channel: JetOutputChannel,
    color_mode: &str,
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
    stderr_is_terminal: bool,
    term: Option<&str>,
    no_color: bool,
    force_color: bool,
    width: i64,
    height: i64,
) -> JetOutputCapabilities {
    jet_output_capabilities_with_mode(
        channel,
        JetOutputColorMode::parse(color_mode),
        stdin_is_terminal,
        stdout_is_terminal,
        stderr_is_terminal,
        term,
        no_color,
        force_color,
        width,
        height,
        true,
    )
}

/// Resolve the one presentation policy from explicit host facts. This is the
/// semantic profile seam: it never reads the process environment or writes a
/// stream. `unicode` is an encoding capability supplied by the host.
#[allow(clippy::too_many_arguments)]
pub(crate) fn jet_output_capabilities_with_mode(
    channel: JetOutputChannel,
    requested_color: JetOutputColorMode,
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
    stderr_is_terminal: bool,
    term: Option<&str>,
    no_color: bool,
    force_color: bool,
    width: i64,
    height: i64,
    unicode: bool,
) -> JetOutputCapabilities {
    let term_is_dumb = term == Some("dumb");
    let human = channel.is_human();
    let stream_is_terminal = match channel {
        JetOutputChannel::HumanStderr => stderr_is_terminal,
        JetOutputChannel::HumanStdout => stdout_is_terminal,
        JetOutputChannel::MachineStdout | JetOutputChannel::Hidden => false,
    };
    // Explicit choice > NO_COLOR > FORCE_COLOR > TTY.  This is the same
    // ladder as the generated report and host terminal policy.
    let color_enabled = human
        && match requested_color {
            JetOutputColorMode::Always => true,
            JetOutputColorMode::Never => false,
            JetOutputColorMode::Auto => {
                if no_color {
                    false
                } else if force_color {
                    true
                } else {
                    stream_is_terminal
                }
            }
        };
    let progress_enabled = human && !term_is_dumb && stream_is_terminal;
    JetOutputCapabilities {
        channel,
        requested_color,
        stdin_is_terminal,
        stdout_is_terminal,
        stderr_is_terminal,
        term_is_dumb,
        no_color,
        force_color,
        json: channel.is_machine(),
        color_enabled,
        unicode_enabled: human && stream_is_terminal && !term_is_dumb && unicode,
        progress_enabled,
        width: usize::try_from(width)
            .ok()
            .filter(|value| *value > 0)
            .unwrap_or(80),
        height: usize::try_from(height)
            .ok()
            .filter(|value| *value > 0)
            .unwrap_or(24),
    }
}

/// Adapt the host's single TTY fact to the Prelude's explicit stream facts.
/// The host has already read `TERM`, `NO_COLOR`, `FORCE_COLOR`, and dimensions.
#[allow(clippy::too_many_arguments)]
pub(crate) fn jet_output_capabilities_from_host(
    channel: JetOutputChannel,
    requested_color: JetOutputColorMode,
    tty: bool,
    term: Option<&str>,
    no_color: bool,
    force_color: bool,
    width: Option<usize>,
    unicode: bool,
) -> JetOutputCapabilities {
    jet_output_capabilities_with_mode(
        channel,
        requested_color,
        tty,
        tty,
        tty,
        term,
        no_color,
        force_color,
        width.unwrap_or(80).min(i64::MAX as usize) as i64,
        24,
        unicode,
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JetOutputTable {
    pub(crate) headers: Vec<String>,
    pub(crate) rows: Vec<Vec<String>>,
}

impl JetOutputTable {
    pub(crate) fn new(mut headers: Vec<String>, mut rows: Vec<Vec<String>>) -> Self {
        headers.truncate(JET_OUTPUT_MAX_TABLE_COLUMNS);
        rows.truncate(JET_OUTPUT_MAX_TABLE_ROWS);
        for header in &mut headers {
            *header = jet_output_bound_text(header);
        }
        for row in &mut rows {
            row.truncate(JET_OUTPUT_MAX_TABLE_COLUMNS);
            for cell in row {
                *cell = jet_output_bound_text(cell);
            }
        }
        Self { headers, rows }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JetOutputStatus {
    pub(crate) label: String,
    pub(crate) value: String,
}

impl JetOutputStatus {
    pub(crate) fn new(label: String, value: String) -> Self {
        Self {
            label: jet_output_bound_text(&label),
            value: jet_output_bound_text(&value),
        }
    }
}

/// A human renderable is intentionally not a machine record.  Machine callers
/// use `jet_output_table_json` or `jet_output_status_json`, so padding, borders,
/// and progress decorations cannot leak into a JSON stream by accident.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum JetHumanRenderable {
    Text(String),
    Table(JetOutputTable),
    Status(JetOutputStatus),
}

pub(crate) fn jet_output_human_only(
    channel: JetOutputChannel,
    renderable: &JetHumanRenderable,
    width: usize,
    unicode: bool,
) -> Option<String> {
    if !channel.is_human() {
        return None;
    }
    Some(match renderable {
        JetHumanRenderable::Text(text) => jet_output_strip_ansi(text),
        JetHumanRenderable::Table(table) => jet_output_table_human(table, width, unicode),
        JetHumanRenderable::Status(status) => jet_output_status_human(status),
    })
}

/// Route one renderable through the resolved profile. Human kinds use the
/// presentation projection; the machine kind uses only the structured JSON
/// projection. Hidden channels produce no bytes.
pub(crate) fn jet_output_render(
    capabilities: JetOutputCapabilities,
    kind: JetOutputKind,
    renderable: &JetHumanRenderable,
) -> Option<String> {
    let channel = capabilities.channel_for(kind);
    if channel.is_hidden() || (kind == JetOutputKind::Progress && !capabilities.progress_enabled) {
        return None;
    }
    if channel.is_machine() {
        return Some(match renderable {
            JetHumanRenderable::Text(text) => format!(
                "{{\"text\":\"{}\"}}",
                jet_output_json_escape(&jet_output_strip_ansi(text))
            ),
            JetHumanRenderable::Table(table) => jet_output_table_json(table),
            JetHumanRenderable::Status(status) => jet_output_status_json(status),
        });
    }
    Some(match renderable {
        JetHumanRenderable::Text(text) => {
            if capabilities.color_enabled {
                text.clone()
            } else {
                jet_output_strip_ansi(text)
            }
        }
        JetHumanRenderable::Table(table) => {
            jet_output_table_human(table, capabilities.width, capabilities.unicode_enabled)
        }
        JetHumanRenderable::Status(status) => jet_output_status_human(status),
    })
}

pub(crate) fn jet_output_style(
    capabilities: JetOutputCapabilities,
    style: &str,
    text: &str,
) -> String {
    if capabilities.color_enabled {
        jet_output_style_force(style, text)
    } else {
        jet_output_strip_ansi(text)
    }
}

pub(crate) fn jet_output_style_force(style: &str, text: &str) -> String {
    let code = match style {
        "black" => Some("30"),
        "red" => Some("31"),
        "green" => Some("32"),
        "yellow" => Some("33"),
        "blue" => Some("34"),
        "magenta" => Some("35"),
        "cyan" => Some("36"),
        "white" => Some("37"),
        "bold" => Some("1"),
        "dim" => Some("2"),
        _ => None,
    };
    code.map_or_else(
        || text.to_string(),
        |code| format!("\x1b[{code}m{text}\x1b[0m"),
    )
}
pub(crate) const JET_OUTPUT_MAX_TASKS: usize = 64;
pub(crate) const JET_OUTPUT_MAX_COLUMNS: usize = 8;
pub(crate) const JET_OUTPUT_MAX_TEXT: usize = 256;
pub(crate) const JET_OUTPUT_MAX_TABLE_COLUMNS: usize = 64;
pub(crate) const JET_OUTPUT_MAX_TABLE_ROWS: usize = 4096;
pub(crate) const JET_OUTPUT_MAX_WIDTH: usize = 256;
pub(crate) const JET_OUTPUT_MAX_FRAME_LINES: usize = 64;
pub(crate) const JET_OUTPUT_MAX_FRAME_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JetOutputProgressState {
    Running,
    Complete,
    Failed,
}

impl JetOutputProgressState {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Complete => "complete",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug)]
pub(crate) struct JetOutputProgressIterState {
    total: Option<usize>,
    description: String,
    format: String,
    started: std::time::Instant,
    count: usize,
    displayed: bool,
    finished: bool,
}

impl JetOutputProgressIterState {
    pub(crate) fn new(description: &str, format: &str, total: Option<usize>) -> Self {
        Self {
            total,
            description: description.to_string(),
            format: format.to_string(),
            started: std::time::Instant::now(),
            count: 0,
            displayed: false,
            finished: false,
        }
    }

    pub(crate) fn advance<E>(
        &mut self,
        color_enabled: bool,
        emit: impl FnOnce(&str) -> Result<(), E>,
        finish: impl FnOnce() -> Result<(), E>,
    ) -> Result<(), E> {
        if self.finished {
            return Ok(());
        }
        self.count = self.count.saturating_add(1);
        if let Some(total) = self.total {
            self.count = self.count.min(total);
        }
        let text = jet_output_progress_legacy(
            &self.description,
            &self.format,
            self.count,
            self.total,
            self.started.elapsed().as_secs_f64(),
            color_enabled,
        );
        emit(&text)?;
        self.displayed = true;
        if self.total == Some(self.count) {
            self.finish(finish)?;
        }
        Ok(())
    }

    pub(crate) fn finish<E>(&mut self, finish: impl FnOnce() -> Result<(), E>) -> Result<(), E> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        if self.displayed {
            finish()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JetOutputProgressColumn {
    pub(crate) key: String,
    pub(crate) value: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct JetOutputProgressTask {
    pub(crate) id: String,
    pub(crate) description: String,
    pub(crate) count: usize,
    pub(crate) total: Option<usize>,
    pub(crate) elapsed: f64,
    pub(crate) state: JetOutputProgressState,
    pub(crate) transient: bool,
    pub(crate) columns: Vec<JetOutputProgressColumn>,
}

impl JetOutputProgressTask {
    pub(crate) fn new(id: &str, description: &str, total: Option<usize>) -> Self {
        let state = (total == Some(0))
            .then_some(JetOutputProgressState::Complete)
            .unwrap_or(JetOutputProgressState::Running);
        Self {
            id: jet_output_bound_text(id),
            description: jet_output_bound_text(description),
            count: 0,
            total,
            elapsed: 0.0,
            state,
            transient: false,
            columns: Vec::new(),
        }
    }
    pub(crate) fn update(&mut self, count: usize, elapsed: f64) {
        self.elapsed = if elapsed.is_finite() {
            elapsed.max(0.0)
        } else {
            0.0
        };
        if let Some(total) = self.total {
            self.count = count.min(total);
            if self.state == JetOutputProgressState::Running && self.count >= total {
                self.count = total;
                self.state = JetOutputProgressState::Complete;
            } else if self.state == JetOutputProgressState::Complete {
                self.count = total;
            }
        } else {
            self.count = count;
        }
    }

    pub(crate) fn complete(&mut self) {
        self.state = JetOutputProgressState::Complete;
        if let Some(total) = self.total {
            self.count = total;
        }
    }

    pub(crate) fn fail(&mut self) {
        self.state = JetOutputProgressState::Failed;
    }

    pub(crate) fn set_transient(&mut self, transient: bool) {
        self.transient = transient;
    }

    pub(crate) fn set_column(&mut self, key: &str, value: &str) {
        let key = jet_output_bound_text(key);
        if key.is_empty() {
            return;
        }
        let value = jet_output_bound_text(value);
        if let Some(column) = self.columns.iter_mut().find(|column| column.key == key) {
            column.value = value;
        } else if self.columns.len() < JET_OUTPUT_MAX_COLUMNS {
            self.columns.push(JetOutputProgressColumn { key, value });
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct JetOutputProgressSnapshot {
    pub(crate) tasks: Vec<JetOutputProgressTask>,
}

impl JetOutputProgressSnapshot {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn upsert(&mut self, task: JetOutputProgressTask) {
        if let Some(current) = self.tasks.iter_mut().find(|current| current.id == task.id) {
            *current = task;
        } else if self.tasks.len() < JET_OUTPUT_MAX_TASKS {
            self.tasks.push(task);
        }
    }

    pub(crate) fn remove(&mut self, id: &str) -> Option<JetOutputProgressTask> {
        let index = self.tasks.iter().position(|task| task.id == id)?;
        Some(self.tasks.remove(index))
    }

    pub(crate) fn render(&self, width: usize, unicode: bool) -> String {
        jet_output_progress_render_tasks(&self.tasks, width, unicode)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JetOutputProgressFrame {
    pub(crate) sequence: u64,
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) clear_lines: usize,
    pub(crate) lines: Vec<String>,
    pub(crate) final_frame: bool,
}

impl JetOutputProgressFrame {
    pub(crate) fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

pub(crate) fn jet_output_progress_legacy(
    description: &str,
    format: &str,
    count: usize,
    total: Option<usize>,
    elapsed: f64,
    color_enabled: bool,
) -> String {
    let count = total.map_or(count, |total| count.min(total));
    let total_text = total.map_or_else(|| "?".to_string(), |total| total.to_string());
    let percent = match total {
        Some(0) => "100".to_string(),
        Some(total) => ((count as u128 * 100) / total as u128).to_string(),
        None => "?".to_string(),
    };
    let elapsed = if elapsed.is_finite() {
        elapsed.max(0.0)
    } else {
        0.0
    };
    let remaining = match total {
        Some(total) if count >= total => "0.00s".to_string(),
        Some(total) if count > 0 => {
            format!("{:.2}s", elapsed * (total - count) as f64 / count as f64)
        }
        Some(_) | None => "?".to_string(),
    };
    let rate = if elapsed > 0.0 {
        format!("{:.2}/s", count as f64 / elapsed)
    } else {
        "?/s".to_string()
    };
    let default_format =
        "{description} {percent}% {count}/{total} elapsed {elapsed} remaining {remaining} rate {rate}";
    let template = if format.is_empty() {
        default_format
    } else {
        format
    };
    let mut rendered = jet_output_bound_raw_text(template);
    let description = jet_output_bound_raw_text(description);
    let count_text = count.to_string();
    let elapsed_text = format!("{elapsed:.2}s");
    for (key, value) in [
        ("description", description.as_str()),
        ("percent", percent.as_str()),
        ("count", count_text.as_str()),
        ("total", total_text.as_str()),
        ("elapsed", elapsed_text.as_str()),
        ("remaining", remaining.as_str()),
        ("rate", rate.as_str()),
    ] {
        rendered = rendered.replace(&format!("{{{key}}}"), value);
    }
    rendered = jet_output_bound_raw_text(&rendered);
    if color_enabled {
        rendered
    } else {
        jet_output_strip_ansi(&rendered)
    }
}

/// Render the bounded progress snapshot. Tasks retain insertion order, and a
/// transient completed task disappears instead of leaving a stale row behind.
pub(crate) fn jet_output_progress_render_tasks(
    tasks: &[JetOutputProgressTask],
    width: usize,
    unicode: bool,
) -> String {
    jet_output_progress_lines(tasks, width, unicode).join("\n")
}

fn jet_output_progress_lines(
    tasks: &[JetOutputProgressTask],
    width: usize,
    unicode: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    for task in tasks.iter().take(JET_OUTPUT_MAX_TASKS) {
        if task.state == JetOutputProgressState::Complete && task.transient {
            continue;
        }
        let mut line = jet_output_progress_line(task, unicode);
        if line.is_empty() {
            line.push(' ');
        }
        lines.extend(jet_output_wrap_cell(&line, width));
    }
    lines
}

fn jet_output_progress_line(task: &JetOutputProgressTask, unicode: bool) -> String {
    let description = jet_output_strip_ansi(&jet_output_bound_text(&task.description));
    let total = task
        .total
        .map_or_else(|| "?".to_string(), |total| total.to_string());
    let count = task.total.map_or(task.count, |total| task.count.min(total));
    let percent = match task.total {
        Some(0) => "100".to_string(),
        Some(total) => ((count as u128 * 100) / total as u128).to_string(),
        None => "?".to_string(),
    };
    let mut line = match task.state {
        JetOutputProgressState::Running => {
            format!("{description} {percent}% {count}/{total}")
        }
        JetOutputProgressState::Complete => format!("{description} {count}/{total}"),
        JetOutputProgressState::Failed => {
            format!("{description} failed at {count}/{total}")
        }
    };
    if task.state == JetOutputProgressState::Complete {
        line.push(' ');
        line.push_str(if unicode { "✓" } else { "done" });
    }
    for column in task.columns.iter().take(JET_OUTPUT_MAX_COLUMNS) {
        line.push(' ');
        line.push_str(&jet_output_strip_ansi(&jet_output_bound_text(&column.key)));
        line.push('=');
        line.push_str(&jet_output_strip_ansi(&jet_output_bound_text(
            &column.value,
        )));
    }
    line
}

/// Construct a deterministic progress frame from an already-selected profile.
/// The frame contains facts only; the host decides whether and how to write it.
pub(crate) fn jet_output_progress_frame(
    capabilities: JetOutputCapabilities,
    sequence: u64,
    previous_line_count: usize,
    snapshot: &JetOutputProgressSnapshot,
) -> Option<JetOutputProgressFrame> {
    if !capabilities.progress_enabled
        || capabilities
            .channel_for(JetOutputKind::Progress)
            .is_hidden()
    {
        return None;
    }
    let width = capabilities.width.clamp(1, JET_OUTPUT_MAX_WIDTH);
    let height = capabilities.height.max(1).min(JET_OUTPUT_MAX_FRAME_LINES);
    let mut lines = jet_output_progress_lines(&snapshot.tasks, width, capabilities.unicode_enabled);
    lines.truncate(height);
    lines = jet_output_bound_frame_lines(lines);
    let final_frame = snapshot
        .tasks
        .iter()
        .take(JET_OUTPUT_MAX_TASKS)
        .filter(|task| !(task.state == JetOutputProgressState::Complete && task.transient))
        .all(|task| task.state != JetOutputProgressState::Running);
    Some(JetOutputProgressFrame {
        sequence,
        width,
        height,
        clear_lines: previous_line_count.min(JET_OUTPUT_MAX_FRAME_LINES),
        lines,
        final_frame,
    })
}

pub(crate) fn jet_output_progress_render_if_enabled(
    capabilities: JetOutputCapabilities,
    snapshot: &JetOutputProgressSnapshot,
) -> Option<String> {
    jet_output_progress_frame(capabilities, 0, 0, snapshot).map(|frame| frame.text())
}

pub(crate) fn jet_output_progress_json(snapshot: &JetOutputProgressSnapshot) -> String {
    let tasks = snapshot
        .tasks
        .iter()
        .take(JET_OUTPUT_MAX_TASKS)
        .map(|task| {
            let columns = task
                .columns
                .iter()
                .take(JET_OUTPUT_MAX_COLUMNS)
                .map(|column| {
                    format!(
                        "\"{}\":\"{}\"",
                        jet_output_json_escape(&jet_output_bound_text(
                            &jet_output_strip_ansi(&column.key),
                        )),
                        jet_output_json_escape(&jet_output_bound_text(
                            &jet_output_strip_ansi(&column.value),
                        ))
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"id\":\"{}\",\"description\":\"{}\",\"count\":{},\"total\":{},\"elapsed\":{},\"state\":\"{}\",\"transient\":{},\"columns\":{{{columns}}}}}",
                jet_output_json_escape(&jet_output_bound_text(&jet_output_strip_ansi(&task.id))),
                jet_output_json_escape(&jet_output_bound_text(&jet_output_strip_ansi(
                    &task.description,
                ))),
                task.count,
                task.total.map_or_else(|| "null".to_string(), |total| total.to_string()),
                if task.elapsed.is_finite() && task.elapsed >= 0.0 {
                    task.elapsed.to_string()
                } else {
                    "0".to_string()
                },
                task.state.as_str(),
                task.transient,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("{{\"tasks\":[{tasks}]}}")
}

pub(crate) fn jet_output_machine_progress_only(
    channel: JetOutputChannel,
    snapshot: &JetOutputProgressSnapshot,
) -> Option<String> {
    channel
        .is_machine()
        .then(|| jet_output_progress_json(snapshot))
}

fn jet_output_bound_raw_text(text: &str) -> String {
    text.chars().take(JET_OUTPUT_MAX_TEXT).collect()
}

fn jet_output_bound_text(text: &str) -> String {
    jet_output_strip_ansi(text)
        .chars()
        .take(JET_OUTPUT_MAX_TEXT)
        .collect()
}

fn jet_output_bound_frame_lines(mut lines: Vec<String>) -> Vec<String> {
    let mut bytes = 0usize;
    let mut retained = Vec::with_capacity(lines.len());
    for mut line in lines.drain(..) {
        let separator = usize::from(!retained.is_empty());
        let available = JET_OUTPUT_MAX_FRAME_BYTES.saturating_sub(bytes + separator);
        if available == 0 {
            break;
        }
        if line.len() > available {
            let mut bounded = String::new();
            for character in line.chars() {
                if bounded.len().saturating_add(character.len_utf8()) > available {
                    break;
                }
                bounded.push(character);
            }
            line = bounded;
        }
        bytes = bytes.saturating_add(separator).saturating_add(line.len());
        retained.push(line);
        if bytes >= JET_OUTPUT_MAX_FRAME_BYTES {
            break;
        }
    }
    retained
}
pub(crate) fn jet_output_status_human(status: &JetOutputStatus) -> String {
    let label = jet_output_strip_ansi(&status.label);
    let value = jet_output_strip_ansi(&status.value);
    if value.is_empty() {
        label
    } else if label.is_empty() {
        value
    } else {
        format!("{label}: {value}")
    }
}

pub(crate) fn jet_output_table_human(
    table: &JetOutputTable,
    width: usize,
    unicode: bool,
) -> String {
    let width = width.max(1);
    let columns = table
        .headers
        .len()
        .max(table.rows.iter().map(Vec::len).max().unwrap_or(0))
        .min(JET_OUTPUT_MAX_TABLE_COLUMNS);
    if columns == 0 {
        return String::new();
    }

    // A bordered table needs three visible cells per column plus its outer
    // corners. At a narrower budget, retain every value in a deterministic
    // wrapped list instead of emitting rows wider than the host requested.
    let minimum_border_width = columns.saturating_mul(3).saturating_add(1);
    if width < minimum_border_width {
        let mut lines = Vec::new();
        let mut append = |cells: &[String]| {
            let line = cells
                .iter()
                .take(columns)
                .map(|cell| jet_output_strip_ansi(cell))
                .collect::<Vec<_>>()
                .join(" | ");
            lines.extend(jet_output_wrap(&line, width));
        };
        if !table.headers.is_empty() {
            append(table.headers.as_slice());
        }
        for row in table.rows.iter().take(JET_OUTPUT_MAX_TABLE_ROWS) {
            append(row.as_slice());
        }
        return lines.join("\n");
    }

    let mut natural = vec![1usize; columns];
    for (index, cell) in table.headers.iter().take(columns).enumerate() {
        natural[index] = natural[index].max(jet_output_visible_width(cell));
    }
    for row in table.rows.iter().take(JET_OUTPUT_MAX_TABLE_ROWS) {
        for (index, cell) in row.iter().take(columns).enumerate() {
            natural[index] = natural[index].max(jet_output_visible_width(cell));
        }
    }

    let separator = if unicode { '│' } else { '|' };
    // Two spaces surround every cell; one separator and two border corners
    // account for the rest of the row. Keep the budget in visible cells so
    // wide Unicode values wrap before they can overflow the requested width.
    let overhead = columns.saturating_mul(3).saturating_add(1);
    let cell_budget = width.saturating_sub(overhead);
    let natural_total = natural.iter().copied().sum::<usize>();
    let widths = if natural_total <= cell_budget {
        natural
    } else {
        jet_output_fit_widths(&natural, cell_budget, columns)
    };

    let mut lines = Vec::new();
    if unicode {
        lines.push(jet_output_border(&widths, '┌', '┬', '┐', '─'));
    } else {
        lines.push(jet_output_border(&widths, '+', '+', '+', '-'));
    }
    if !table.headers.is_empty() {
        lines.extend(jet_output_cells(
            table.headers.as_slice(),
            &widths,
            separator,
        ));
        if unicode {
            lines.push(jet_output_border(&widths, '├', '┼', '┤', '─'));
        } else {
            lines.push(jet_output_border(&widths, '+', '+', '+', '-'));
        }
    }
    for row in table.rows.iter().take(JET_OUTPUT_MAX_TABLE_ROWS) {
        lines.extend(jet_output_cells(row.as_slice(), &widths, separator));
    }
    if unicode {
        lines.push(jet_output_border(&widths, '└', '┴', '┘', '─'));
    } else {
        lines.push(jet_output_border(&widths, '+', '+', '+', '-'));
    }
    lines.join("\n")
}

fn jet_output_fit_widths(natural: &[usize], budget: usize, columns: usize) -> Vec<usize> {
    let mut widths = vec![1usize; columns];
    let mut remaining = budget.saturating_sub(columns);
    // ponytail: proportional allocation is unnecessary for terminal output;
    // give each column its natural width in declaration order until the budget
    // is spent.  A weighted allocator can replace this only if layout evidence
    // shows a real readability loss.
    while remaining != 0 {
        let mut grew = false;
        for (index, width) in widths.iter_mut().enumerate() {
            if *width < natural[index] && remaining != 0 {
                *width += 1;
                remaining -= 1;
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    widths
}

fn jet_output_border(widths: &[usize], left: char, joint: char, right: char, fill: char) -> String {
    let mut line = String::new();
    line.push(left);
    for (index, width) in widths.iter().enumerate() {
        if index != 0 {
            line.push(joint);
        }
        line.extend(std::iter::repeat_n(fill, width.saturating_add(2)));
    }
    line.push(right);
    line
}

fn jet_output_cells(cells: &[String], widths: &[usize], separator: char) -> Vec<String> {
    let wrapped = widths
        .iter()
        .enumerate()
        .map(|(index, width)| {
            cells
                .get(index)
                .map(|cell| jet_output_wrap_cell(cell, *width))
                .unwrap_or_else(|| vec![String::new()])
        })
        .collect::<Vec<_>>();
    let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
    (0..height)
        .map(|line_index| {
            let mut line = String::new();
            line.push(separator);
            for (index, width) in widths.iter().enumerate() {
                if index != 0 {
                    line.push(separator);
                }
                line.push(' ');
                let cell = wrapped[index]
                    .get(line_index)
                    .map(String::as_str)
                    .unwrap_or("");
                line.push_str(cell);
                line.extend(std::iter::repeat_n(
                    ' ',
                    width
                        .saturating_sub(jet_output_visible_width(cell))
                        .saturating_add(1),
                ));
            }
            line.push(separator);
            line
        })
        .collect()
}

pub(crate) fn jet_output_wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let text = jet_output_strip_ansi(text);
    let mut lines = Vec::new();
    for logical in text.split('\n') {
        if logical.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut line = String::new();
        let mut line_width = 0usize;
        let mut emitted = false;
        jet_output_for_each_cluster(logical, |cluster, cluster_width| {
            if cluster_width == 0 {
                if line_width < width {
                    line.push_str(cluster);
                }
                return;
            }
            if cluster_width > width {
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                    line_width = 0;
                }
                lines.push("?".to_string());
                emitted = true;
                return;
            }
            if line_width != 0 && line_width.saturating_add(cluster_width) > width {
                lines.push(std::mem::take(&mut line));
                line_width = 0;
            }
            line.push_str(cluster);
            line_width = line_width.saturating_add(cluster_width);
        });
        if !line.is_empty() || !emitted {
            lines.push(line);
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn jet_output_wrap_cell(text: &str, width: usize) -> Vec<String> {
    jet_output_wrap(text, width)
}

pub(crate) fn jet_output_visible_width(text: &str) -> usize {
    jet_output_strip_ansi(text)
        .split('\n')
        .map(|line| {
            let mut width = 0usize;
            jet_output_for_each_cluster(line, |_, cluster_width| {
                width = width.saturating_add(cluster_width);
            });
            width
        })
        .max()
        .unwrap_or(0)
}

pub(crate) fn jet_output_char_width(character: char) -> usize {
    if character.is_control() || jet_output_is_combining(character) {
        0
    } else if jet_output_is_wide(character) {
        2
    } else {
        1
    }
}

fn jet_output_is_combining(character: char) -> bool {
    matches!(
        character as u32,
        0x0300..=0x036f
            | 0x0483..=0x0489
            | 0x0591..=0x05bd
            | 0x05bf
            | 0x05c1..=0x05c2
            | 0x05c4..=0x05c5
            | 0x0610..=0x061a
            | 0x064b..=0x065f
            | 0x0670
            | 0x06d6..=0x06dc
            | 0x06df..=0x06e4
            | 0x06e7..=0x06e8
            | 0x06ea..=0x06ed
            | 0x0711
            | 0x0730..=0x074a
            | 0x07a6..=0x07b0
            | 0x07eb..=0x07f3
            | 0x0816..=0x0819
            | 0x081b..=0x0823
            | 0x0825..=0x0827
            | 0x0829..=0x082d
            | 0x0859..=0x085b
            | 0x08d3..=0x0903
            | 0x093a..=0x093c
            | 0x093e..=0x094f
            | 0x0951..=0x0957
            | 0x0962..=0x0963
            | 0x0981..=0x0984
            | 0x09be..=0x09c4
            | 0x09c7..=0x09c8
            | 0x09cb..=0x09cd
            | 0x09d7
            | 0x09e2..=0x09e3
            | 0x0a01..=0x0a03
            | 0x0a3c
            | 0x0a3e..=0x0a42
            | 0x0a47..=0x0a48
            | 0x0a4b..=0x0a4d
            | 0x0a51
            | 0x0a70..=0x0a71
            | 0x0a75
            | 0x0abc
            | 0x0abe..=0x0ac5
            | 0x0ac7..=0x0ac9
            | 0x0acb..=0x0acd
            | 0x0ae2..=0x0ae3
            | 0x0b01..=0x0b03
            | 0x0b3c
            | 0x0b3e..=0x0b44
            | 0x0b47..=0x0b48
            | 0x0b4b..=0x0b4d
            | 0x0b56..=0x0b57
            | 0x0b62..=0x0b63
            | 0x0c00..=0x0c04
            | 0x0c3e..=0x0c44
            | 0x0c46..=0x0c48
            | 0x0c4a..=0x0c4d
            | 0x0c55..=0x0c56
            | 0x0c62..=0x0c63
            | 0x0d00..=0x0d03
            | 0x0d3b..=0x0d44
            | 0x0d46..=0x0d48
            | 0x0d4a..=0x0d4d
            | 0x0d57
            | 0x0d62..=0x0d63
            | 0x0e31
            | 0x0e34..=0x0e3a
            | 0x0e47..=0x0e4e
            | 0x0eb1
            | 0x0eb4..=0x0ebc
            | 0x0ec8..=0x0ecd
            | 0x0f18
            | 0x0f35
            | 0x0f37
            | 0x0f39
            | 0x0f71..=0x0f84
            | 0x0f86..=0x0f87
            | 0x0f8d..=0x0fbc
            | 0x0fc6
            | 0x102b..=0x103e
            | 0x1056..=0x1059
            | 0x105e..=0x1060
            | 0x1062..=0x1064
            | 0x1067..=0x106d
            | 0x1071..=0x1074
            | 0x1082
            | 0x1084
            | 0x1087..=0x108d
            | 0x108f
            | 0x109a..=0x109d
            | 0x135d..=0x135f
            | 0x1712..=0x1714
            | 0x1732..=0x1734
            | 0x1752..=0x1753
            | 0x1772..=0x1773
            | 0x17b4..=0x17d3
            | 0x17dd
            | 0x180b..=0x180f
            | 0x1885..=0x1886
            | 0x18a9
            | 0x1920..=0x193b
            | 0x1a17..=0x1a1b
            | 0x1a55..=0x1a5e
            | 0x1a60
            | 0x1a62..=0x1a6f
            | 0x1a70..=0x1a7c
            | 0x1a7f
            | 0x1ab0..=0x1aff
            | 0x1b00..=0x1b04
            | 0x1b34
            | 0x1b36..=0x1b44
            | 0x1b6b..=0x1b73
            | 0x1b80..=0x1b82
            | 0x1ba1..=0x1bad
            | 0x1be6..=0x1bf3
            | 0x1c24..=0x1c37
            | 0x1cd0..=0x1cd2
            | 0x1cd4..=0x1ce8
            | 0x1ced
            | 0x1cf4
            | 0x1cf7..=0x1cf9
            | 0x1cfb..=0x1cfc
            | 0x1d167..=0x1d1ff
            | 0x1dc00..=0x1dfff
            | 0x20d0..=0x20f0
            | 0x2cef..=0x2cf1
            | 0x2de0..=0x2dff
            | 0x302a..=0x302f
            | 0x3099..=0x309a
            | 0xa66f..=0xa672
            | 0xa674..=0xa67d
            | 0xa69e..=0xa69f
            | 0xa6f0..=0xa6f1
            | 0xa802
            | 0xa806
            | 0xa80b
            | 0xa823..=0xa827
            | 0xa82c
            | 0xa880..=0xa881
            | 0xa8b4..=0xa8c5
            | 0xa8e0..=0xa8f1
            | 0xa8ff
            | 0xa926..=0xa92f
            | 0xa947..=0xa953
            | 0xa980..=0xa983
            | 0xa9b3..=0xa9c0
            | 0xa9c6
            | 0xa9e5
            | 0xaa29..=0xaa3f
            | 0xaa43
            | 0xaa4c..=0xaa4d
            | 0xaa7b..=0xaa7d
            | 0xaab0
            | 0xaab2..=0xaab4
            | 0xaab7..=0xaab8
            | 0xaabe..=0xaabf
            | 0xaac1
            | 0xaaeb..=0xaaef
            | 0xaaf5..=0xaaf6
            | 0xabe3..=0xabe4
            | 0xabe6..=0xabe7
            | 0xabe9
            | 0xabec
            | 0xabf0..=0xabf1
            | 0xfe00..=0xfe0f
            | 0xfe20..=0xfe2f
            | 0xff9e..=0xff9f
            | 0x1f3fb..=0x1f3ff
    )
}

fn jet_output_is_wide(character: char) -> bool {
    matches!(
        character as u32,
        0x1100..=0x115f
            | 0x231a..=0x231b
            | 0x2329..=0x232a
            | 0x23e9..=0x23ec
            | 0x23f0
            | 0x23f3
            | 0x25fd..=0x25fe
            | 0x2614..=0x2615
            | 0x2648..=0x2653
            | 0x267f
            | 0x2693
            | 0x26a1
            | 0x26aa..=0x26ab
            | 0x26bd..=0x26be
            | 0x26c4..=0x26c5
            | 0x26ce
            | 0x26d4
            | 0x26ea
            | 0x26f2..=0x26f3
            | 0x26f5
            | 0x26fa
            | 0x26fd
            | 0x2705
            | 0x270a..=0x270b
            | 0x2728
            | 0x274c
            | 0x274e
            | 0x2753..=0x2755
            | 0x2757
            | 0x2795..=0x2797
            | 0x27b0
            | 0x27bf
            | 0x2b1b..=0x2b1c
            | 0x2b50
            | 0x2b55
            | 0x2e80..=0x2ffb
            | 0x3000..=0x303e
            | 0x3041..=0x3247
            | 0x3250..=0x4dbf
            | 0x4e00..=0xa4c6
            | 0xa960..=0xa97c
            | 0xac00..=0xd7a3
            | 0xf900..=0xfaff
            | 0xfe10..=0xfe19
            | 0xfe30..=0xfe6b
            | 0xff01..=0xff60
            | 0xffe0..=0xffe6
            | 0x1f300..=0x1f64f
            | 0x1f680..=0x1f6ff
            | 0x1f900..=0x1f9ff
            | 0x20000..=0x3fffd
    )
}

fn jet_output_for_each_cluster(text: &str, mut visit: impl FnMut(&str, usize)) {
    let mut chars = text.char_indices().peekable();
    while let Some((start, first)) = chars.next() {
        let mut end = start + first.len_utf8();
        let mut width = jet_output_char_width(first);
        if jet_output_is_regional_indicator(first) {
            if let Some((_, next)) = chars.peek().copied() {
                if jet_output_is_regional_indicator(next) {
                    if let Some((next_start, next)) = chars.next() {
                        end = next_start + next.len_utf8();
                        width = 2;
                    }
                }
            }
        } else {
            loop {
                while let Some((_, next)) = chars.peek().copied() {
                    if jet_output_is_combining(next) {
                        let (next_start, next) = chars.next().expect("peeked cluster extension");
                        end = next_start + next.len_utf8();
                    } else {
                        break;
                    }
                }
                let Some((_, '\u{200d}')) = chars.peek().copied() else {
                    break;
                };
                let (join_start, joiner) = chars.next().expect("peeked grapheme joiner");
                end = join_start + joiner.len_utf8();
                let Some((next_start, next)) = chars.next() else {
                    break;
                };
                end = next_start + next.len_utf8();
                width = width.max(jet_output_char_width(next));
            }
        }
        visit(&text[start..end], width);
    }
}

fn jet_output_is_regional_indicator(character: char) -> bool {
    matches!(character as u32, 0x1f1e6..=0x1f1ff)
}

pub(crate) fn jet_output_strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut state = 0u8;
    for character in text.chars() {
        match state {
            0 if character == '\x1b' => state = 1,
            0 => out.push(character),
            1 if character == '[' => state = 2,
            1 if character == ']' => state = 3,
            1 if character.is_ascii_alphabetic() => state = 0,
            1 => state = 0,
            2 if ('@'..='~').contains(&character) => state = 0,
            2 => {}
            3 if character == '\x07' => state = 0,
            3 if character == '\x1b' => state = 4,
            3 => {}
            4 if character == '\\' => state = 0,
            4 => state = 3,
            _ => state = 0,
        }
    }
    out
}

pub(crate) fn jet_output_table_json(table: &JetOutputTable) -> String {
    let headers = table
        .headers
        .iter()
        .take(JET_OUTPUT_MAX_TABLE_COLUMNS)
        .map(|header| format!("\"{}\"", jet_output_json_escape(header)))
        .collect::<Vec<_>>()
        .join(",");
    let rows = table
        .rows
        .iter()
        .take(JET_OUTPUT_MAX_TABLE_ROWS)
        .map(|row| {
            let cells = row
                .iter()
                .take(JET_OUTPUT_MAX_TABLE_COLUMNS)
                .map(|cell| format!("\"{}\"", jet_output_json_escape(cell)))
                .collect::<Vec<_>>()
                .join(",");
            format!("[{cells}]")
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("{{\"headers\":[{headers}],\"rows\":[{rows}]}}")
}

pub(crate) fn jet_output_status_json(status: &JetOutputStatus) -> String {
    format!(
        "{{\"label\":\"{}\",\"value\":\"{}\"}}",
        jet_output_json_escape(&status.label),
        jet_output_json_escape(&status.value)
    )
}

pub(crate) fn jet_output_machine_table_only(
    channel: JetOutputChannel,
    table: &JetOutputTable,
) -> Option<String> {
    channel.is_machine().then(|| jet_output_table_json(table))
}

pub(crate) fn jet_output_machine_status_only(
    channel: JetOutputChannel,
    status: &JetOutputStatus,
) -> Option<String> {
    channel.is_machine().then(|| jet_output_status_json(status))
}

fn jet_output_json_escape(text: &str) -> String {
    let text = jet_output_strip_ansi(text);
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                out.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => out.push(character),
        }
    }
    out
}

#[cfg(test)]
mod human_output_tests {
    use super::*;

    fn terminal_profile(channel: JetOutputChannel) -> JetOutputCapabilities {
        jet_output_capabilities_with_mode(
            channel,
            JetOutputColorMode::Auto,
            true,
            true,
            true,
            Some("xterm-256color"),
            false,
            false,
            80,
            24,
            true,
        )
    }

    #[test]
    fn auto_color_uses_no_color_then_force_color_then_tty() {
        let redirected = jet_output_capabilities_with_mode(
            JetOutputChannel::HumanStdout,
            JetOutputColorMode::Auto,
            false,
            false,
            false,
            Some("xterm-256color"),
            false,
            false,
            80,
            24,
            true,
        );
        assert!(!redirected.color_enabled);

        let forced = jet_output_capabilities_with_mode(
            JetOutputChannel::HumanStdout,
            JetOutputColorMode::Auto,
            false,
            false,
            false,
            Some("xterm-256color"),
            false,
            true,
            80,
            24,
            true,
        );
        assert!(forced.color_enabled);

        let no_color = jet_output_capabilities_with_mode(
            JetOutputChannel::HumanStdout,
            JetOutputColorMode::Auto,
            false,
            true,
            true,
            Some("xterm-256color"),
            true,
            false,
            80,
            24,
            true,
        );
        assert!(!no_color.color_enabled);

        let explicit = jet_output_capabilities_with_mode(
            JetOutputChannel::HumanStdout,
            JetOutputColorMode::Always,
            false,
            false,
            false,
            Some("dumb"),
            true,
            false,
            80,
            24,
            true,
        );
        assert!(explicit.color_enabled);
        assert!(!explicit.progress_enabled);
    }

    #[test]
    fn machine_profile_hides_human_kinds_and_ansi() {
        let profile = terminal_profile(JetOutputChannel::MachineStdout);
        assert!(profile.machine());
        assert!(profile.json);
        assert!(!profile.color_enabled);
        assert!(!profile.progress_enabled);
        for kind in [
            JetOutputKind::Renderable,
            JetOutputKind::Status,
            JetOutputKind::Log,
            JetOutputKind::Progress,
        ] {
            assert_eq!(profile.channel_for(kind), JetOutputChannel::Hidden);
        }
        assert_eq!(
            profile.channel_for(JetOutputKind::Machine),
            JetOutputChannel::MachineStdout
        );

        let renderable = JetHumanRenderable::Text("\x1b[31msecret\x1b[0m".to_string());
        assert!(
            jet_output_human_only(JetOutputChannel::MachineStdout, &renderable, 80, true).is_none()
        );
        let table = JetOutputTable::new(
            vec!["key".to_string()],
            vec![vec!["\x1b[32mvalue\x1b[0m".to_string()]],
        );
        let json = jet_output_machine_table_only(JetOutputChannel::MachineStdout, &table)
            .expect("machine table");
        assert!(!json.contains('\x1b'));
    }

    #[test]
    fn redirected_profile_is_plain_and_progress_free() {
        let profile = jet_output_capabilities_from_host(
            JetOutputChannel::HumanStdout,
            JetOutputColorMode::Auto,
            false,
            Some("xterm"),
            false,
            false,
            None,
            true,
        );
        assert!(!profile.color_enabled);
        assert!(!profile.unicode_enabled);
        assert!(!profile.progress_enabled);
        let text = JetHumanRenderable::Text("\x1b[31mplain\x1b[0m".to_string());
        assert_eq!(
            jet_output_render(profile, JetOutputKind::Renderable, &text),
            Some("plain".to_string())
        );
        let snapshot = JetOutputProgressSnapshot::new();
        assert!(jet_output_progress_render_if_enabled(profile, &snapshot).is_none());
    }

    #[test]
    fn visible_width_and_narrow_table_use_unicode_cells() {
        assert_eq!(jet_output_visible_width("\x1b[1m東京\x1b[0m"), 4);
        assert_eq!(jet_output_visible_width("e\u{301}"), 1);
        assert_eq!(jet_output_wrap("東京abc", 4), vec!["東京", "abc"]);

        let table = JetOutputTable::new(
            vec!["name".to_string(), "value".to_string()],
            vec![vec!["東京".to_string(), "a long value".to_string()]],
        );
        let rendered = jet_output_table_human(&table, 16, true);
        assert!(!rendered.contains('\x1b'));
        assert!(rendered
            .lines()
            .all(|line| jet_output_visible_width(line) <= 16));
        let narrow = jet_output_table_human(&table, 6, false);
        assert!(narrow
            .lines()
            .all(|line| jet_output_visible_width(line) <= 6));
    }

    #[test]
    fn progress_snapshot_has_bounded_deterministic_frames() {
        let profile = terminal_profile(JetOutputChannel::HumanStdout);
        let mut running = JetOutputProgressTask::new("compile", "Compile", Some(10));
        running.update(4, 1.5);
        running.set_column("phase", "codegen");
        let mut unknown = JetOutputProgressTask::new("upload", "Upload", None);
        unknown.update(2, 0.25);
        let mut transient = JetOutputProgressTask::new("done", "Done", Some(1));
        transient.complete();
        transient.set_transient(true);

        let mut snapshot = JetOutputProgressSnapshot::new();
        snapshot.upsert(running);
        snapshot.upsert(unknown);
        snapshot.upsert(transient);
        let frame = jet_output_progress_frame(profile, 7, 3, &snapshot).expect("progress frame");
        assert_eq!(frame.sequence, 7);
        assert_eq!(frame.clear_lines, 3);
        assert!(!frame.final_frame);
        assert!(!frame.text().contains("Done"));
        assert!(frame.text().contains("Compile"));
        assert!(frame.text().contains("phase=codegen"));
        assert!(frame.lines.len() <= JET_OUTPUT_MAX_FRAME_LINES);
        assert!(frame.text().len() <= JET_OUTPUT_MAX_FRAME_BYTES);

        let json = jet_output_machine_progress_only(JetOutputChannel::MachineStdout, &snapshot)
            .expect("machine progress");
        assert!(json.contains("\"state\":\"running\""));
        assert!(!json.contains('\x1b'));
    }
}
