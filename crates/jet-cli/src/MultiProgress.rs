//! Typed, deterministic progress facts and frames.
//!
//! `MultiProgress` owns state only.  It never probes a terminal, reads an
//! environment variable, starts a worker, or writes a stream.  A borrowed
//! [`OutputProfile`] decides whether a human frame is visible; callers that
//! need machine output use [`MultiProgress::snapshot`] or
//! [`MultiProgress::facts`].

use crate::OutputProfile::OutputProfile;
use std::fmt;

/// Maximum number of tasks retained by one progress surface.
pub const MAX_PROGRESS_TASKS: usize = 128;
/// Maximum UTF-8 bytes in one task label.
pub const MAX_PROGRESS_LABEL_BYTES: usize = 256;
/// Maximum custom columns on one task.
pub const MAX_PROGRESS_COLUMNS: usize = 16;
/// Maximum UTF-8 bytes in one custom column name.
pub const MAX_PROGRESS_COLUMN_NAME_BYTES: usize = 64;
/// Maximum UTF-8 bytes in one custom column value.
pub const MAX_PROGRESS_COLUMN_VALUE_BYTES: usize = 256;
/// Maximum UTF-8 bytes in one task completion/failure message.
pub const MAX_PROGRESS_MESSAGE_BYTES: usize = 256;
/// Maximum UTF-8 bytes in one rendered frame.
pub const MAX_PROGRESS_FRAME_BYTES: usize = 64 * 1024;
/// Maximum visible columns used for one rendered line.
pub const MAX_PROGRESS_WIDTH: usize = 256;

/// A stable insertion-order task identifier.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TaskId(usize);

impl TaskId {
    /// Construct an identifier from its zero-based insertion index.
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    /// Return the zero-based insertion index.
    pub const fn index(self) -> usize {
        self.0
    }
}

impl From<usize> for TaskId {
    fn from(index: usize) -> Self {
        Self(index)
    }
}

/// A task total that remains explicit when work cannot be counted up front.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ProgressTotal {
    /// The task has a finite total.
    Known(u64),
    /// The task reports work without a finite total.
    Unknown,
}

impl ProgressTotal {
    /// Return the machine-friendly optional representation.
    pub const fn as_option(self) -> Option<u64> {
        match self {
            Self::Known(total) => Some(total),
            Self::Unknown => None,
        }
    }

    /// Return true when the total is finite.
    pub const fn is_known(self) -> bool {
        matches!(self, Self::Known(_))
    }
}

impl From<Option<u64>> for ProgressTotal {
    fn from(total: Option<u64>) -> Self {
        match total {
            Some(total) => Self::Known(total),
            None => Self::Unknown,
        }
    }
}

/// The lifecycle state of one progress task.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ProgressState {
    /// Work may still advance.
    Running,
    /// The caller explicitly completed the task.
    Finished,
    /// The caller explicitly reported a failure.
    Failed,
    /// The caller explicitly cancelled the task.
    Cancelled,
}

impl ProgressState {
    /// Return true for every terminal state.
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }

    /// Return the stable lowercase frame/machine spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Finished => "finished",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// One bounded custom task column.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgressColumn {
    /// Stable display name.
    pub name: String,
    /// Current display value.
    pub value: String,
}

impl ProgressColumn {
    /// Construct a column.  Bounds are enforced when the column is attached to
    /// a task; [`Self::try_new`] validates at construction time as well.
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }

    /// Construct and validate a column.
    pub fn try_new(
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, ProgressError> {
        let column = Self::new(name, value);
        validate_column(&column)?;
        Ok(column)
    }

    /// Return the column name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the current column value.
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// Options used by [`MultiProgress::add_task_with_options`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TaskOptions {
    /// `Some(total)` is known work; `None` is an unknown total.
    pub total: Option<u64>,
    /// Hide this task after any terminal state in human frames.
    pub transient: bool,
    /// Initial custom columns, in the order in which they render.
    pub columns: Vec<ProgressColumn>,
}

impl TaskOptions {
    /// Set a finite total.
    pub fn known(total: u64) -> Self {
        Self {
            total: Some(total),
            ..Self::default()
        }
    }

    /// Set an indeterminate total.
    pub fn unknown() -> Self {
        Self::default()
    }

    /// Make terminal completion transient.
    pub fn transient(mut self, transient: bool) -> Self {
        self.transient = transient;
        self
    }

    /// Append one initial column.
    pub fn column(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.columns.push(ProgressColumn::new(name, value));
        self
    }
}

/// A structured, machine-safe fact for one task.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgressFact {
    /// Stable insertion-order identity.
    pub id: TaskId,
    /// Original task label, without ANSI decoration.
    pub label: String,
    /// Completed units.
    pub completed: u64,
    /// Known or unknown total.
    pub total: ProgressTotal,
    /// Explicit lifecycle state.
    pub state: ProgressState,
    /// Whether terminal states are transient in human frames.
    pub transient: bool,
    /// Ordered custom columns.
    pub columns: Vec<ProgressColumn>,
    /// Optional explicit finish/failure/cancel message.
    pub message: Option<String>,
}

impl ProgressFact {
    /// Return the completed-unit count under the common `position` name.
    pub const fn position(&self) -> u64 {
        self.completed
    }

    /// Return the optional total used by structured consumers.
    pub const fn total_value(&self) -> Option<u64> {
        self.total.as_option()
    }

    /// Return true after an explicit terminal transition.
    pub const fn is_terminal(&self) -> bool {
        self.state.is_terminal()
    }
}

/// One state transition accepted by [`MultiProgress::apply`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgressUpdate {
    /// Add a non-negative number of completed units.
    Increment(u64),
    /// Set completed units, clamped to a known total.
    SetPosition(u64),
    /// Replace the known/unknown total.
    SetTotal(Option<u64>),
    /// Insert or replace one custom column.
    SetColumn(ProgressColumn),
    /// Explicitly complete the task.
    Finish,
    /// Explicitly fail the task, optionally with a bounded message.
    Fail { message: Option<String> },
    /// Explicitly cancel the task, optionally with a bounded message.
    Cancel { message: Option<String> },
}

/// A bounded progress operation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgressError {
    /// A task, label, column, message, or frame exceeded its bound.
    Limit { what: &'static str, limit: usize },
    /// A user-facing label or value was empty or contained a control byte.
    InvalidText { what: &'static str },
    /// The task identifier is not present in this surface.
    UnknownTask(TaskId),
    /// A terminal task cannot receive another work transition.
    TaskClosed(TaskId),
}

impl fmt::Display for ProgressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit { what, limit } => write!(formatter, "{what} exceeds the {limit}-byte/item limit"),
            Self::InvalidText { what } => write!(formatter, "{what} is empty or contains a control character"),
            Self::UnknownTask(id) => write!(formatter, "unknown progress task {}", id.index()),
            Self::TaskClosed(id) => write!(formatter, "progress task {} is already closed", id.index()),
        }
    }
}

impl std::error::Error for ProgressError {}

#[derive(Clone, Debug)]
struct TaskState {
    id: TaskId,
    label: String,
    completed: u64,
    total: ProgressTotal,
    state: ProgressState,
    transient: bool,
    columns: Vec<ProgressColumn>,
    message: Option<String>,
}

impl TaskState {
    fn fact(&self) -> ProgressFact {
        ProgressFact {
            id: self.id,
            label: self.label.clone(),
            completed: self.completed,
            total: self.total,
            state: self.state,
            transient: self.transient,
            columns: self.columns.clone(),
            message: self.message.clone(),
        }
    }
}

fn complete_known_task(task: &mut TaskState) {
    if task.state != ProgressState::Running {
        return;
    }
    let ProgressTotal::Known(total) = task.total else {
        return;
    };
    if task.completed >= total {
        task.completed = total;
        task.state = ProgressState::Finished;
        task.message = None;
    }
}

/// A deterministic visible frame.  Lines are kept separately so a caller can
/// apply [`ProgressFrameDiff`] without parsing terminal control sequences.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgressFrame {
    /// Complete frame text, without a trailing newline.
    pub text: String,
    /// The same text split into deterministic rows.
    pub lines: Vec<String>,
    /// UTF-8 byte length of [`Self::text`].
    pub bytes: usize,
}

impl ProgressFrame {
    fn from_lines(lines: Vec<String>) -> Result<Self, ProgressError> {
        let text = lines.join("\n");
        if text.len() > MAX_PROGRESS_FRAME_BYTES {
            return Err(ProgressError::Limit {
                what: "progress frame bytes",
                limit: MAX_PROGRESS_FRAME_BYTES,
            });
        }
        Ok(Self {
            bytes: text.len(),
            text,
            lines,
        })
    }

    /// Return the complete frame text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Return rows in deterministic task order.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Return true when there are no visible tasks.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Diff this frame against a later frame.
    pub fn diff(&self, next: &Self) -> ProgressFrameDiff {
        diff_frames(Some(self), Some(next))
    }
}

/// One changed row in a deterministic frame diff.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameLineChange {
    /// Zero-based row index in the new frame.
    pub index: usize,
    /// Complete replacement text for that row.
    pub text: String,
}

/// A side-effect-free frame diff.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgressFrameDiff {
    /// Whether rows or row count changed.
    pub changed: bool,
    /// Number of rows in the old frame.
    pub previous_lines: usize,
    /// Number of rows in the new frame.
    pub current_lines: usize,
    /// Changed/new rows in ascending row order.
    pub changed_lines: Vec<FrameLineChange>,
    /// Rows that must be removed from the old tail.
    pub removed_lines: usize,
    /// Complete new frame text, useful to simple writers.
    pub text: String,
}

impl ProgressFrameDiff {
    /// Return a zero-change diff.
    pub fn empty() -> Self {
        Self {
            changed: false,
            previous_lines: 0,
            current_lines: 0,
            changed_lines: Vec::new(),
            removed_lines: 0,
            text: String::new(),
        }
    }

    /// Return true when no terminal update is needed.
    pub const fn is_empty(&self) -> bool {
        !self.changed
    }

    /// Number of changed/new rows.
    pub fn changed_line_count(&self) -> usize {
        self.changed_lines.len()
    }
}

/// Compare two optional frames without terminal I/O.
///
/// `None` means that no human frame is visible.  Passing `None` for both
/// frames produces an empty diff; passing a visible frame followed by `None`
/// describes removal of all old rows without inventing output bytes.
pub fn diff_frames(
    previous: Option<&ProgressFrame>,
    current: Option<&ProgressFrame>,
) -> ProgressFrameDiff {
    let previous_lines = previous.map_or(&[][..], |frame| frame.lines.as_slice());
    let current_lines = current.map_or(&[][..], |frame| frame.lines.as_slice());
    let common = previous_lines.len().min(current_lines.len());
    let mut changed_lines = Vec::new();
    for index in 0..current_lines.len() {
        if index >= common || previous_lines[index] != current_lines[index] {
            changed_lines.push(FrameLineChange {
                index,
                text: current_lines[index].clone(),
            });
        }
    }
    let removed_lines = previous_lines.len().saturating_sub(current_lines.len());
    let changed = previous_lines != current_lines;
    ProgressFrameDiff {
        changed,
        previous_lines: previous_lines.len(),
        current_lines: current_lines.len(),
        changed_lines,
        removed_lines,
        text: current.map_or_else(String::new, |frame| frame.text.clone()),
    }
}

/// Structured facts plus an optional human frame from one observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgressObservation {
    /// All tasks, including transient terminal tasks.
    pub facts: Vec<ProgressFact>,
    /// No frame is returned when the profile suppresses progress.
    pub frame: Option<ProgressFrame>,
    /// Diff from the preceding visible observation.
    pub diff: ProgressFrameDiff,
}

/// A mutable multi-task progress state machine borrowed over one capability
/// decision.  It is deliberately single-threaded; callers serialize updates
/// in their command's normal control flow.
pub struct MultiProgress<'profile> {
    profile: &'profile OutputProfile,
    tasks: Vec<TaskState>,
    previous_frame: Option<ProgressFrame>,
}

impl<'profile> MultiProgress<'profile> {
    /// Create an empty surface using only the caller's capability profile.
    pub fn new(profile: &'profile OutputProfile) -> Self {
        Self {
            profile,
            tasks: Vec::new(),
            previous_frame: None,
        }
    }

    /// Return the borrowed capability decision.
    pub fn profile(&self) -> &'profile OutputProfile {
        self.profile
    }

    /// Return true when this profile permits human progress frames.
    pub fn progress_enabled(&self) -> bool {
        self.profile.progress_enabled()
    }

    /// Return true when frames are suppressed by the borrowed profile.
    pub fn is_suppressed(&self) -> bool {
        !self.progress_enabled()
    }

    /// Return the number of retained tasks.
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    /// Return true when no task has been added.
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Add a task with a known or unknown total.
    pub fn add_task(
        &mut self,
        label: impl Into<String>,
        total: Option<u64>,
    ) -> Result<TaskId, ProgressError> {
        self.add_task_with_options(
            label,
            TaskOptions {
                total,
                ..TaskOptions::default()
            },
        )
    }

    /// Add a task with a finite total.
    pub fn add_known(
        &mut self,
        label: impl Into<String>,
        total: u64,
    ) -> Result<TaskId, ProgressError> {
        self.add_task(label, Some(total))
    }

    /// Add a task whose total is not known up front.
    pub fn add_unknown(&mut self, label: impl Into<String>) -> Result<TaskId, ProgressError> {
        self.add_task(label, None)
    }

    /// Short alias for [`Self::add_task`].
    pub fn add(
        &mut self,
        label: impl Into<String>,
        total: Option<u64>,
    ) -> Result<TaskId, ProgressError> {
        self.add_task(label, total)
    }

    /// Add one task and validate every bounded initial field.
    pub fn add_task_with_options(
        &mut self,
        label: impl Into<String>,
        options: TaskOptions,
    ) -> Result<TaskId, ProgressError> {
        if self.tasks.len() >= MAX_PROGRESS_TASKS {
            return Err(ProgressError::Limit {
                what: "progress tasks",
                limit: MAX_PROGRESS_TASKS,
            });
        }
        let label = label.into();
        validate_label(&label)?;
        validate_columns(&options.columns)?;
        let id = TaskId::new(self.tasks.len());
        let total = options.total.into();
        let state = matches!(total, ProgressTotal::Known(0))
            .then_some(ProgressState::Finished)
            .unwrap_or(ProgressState::Running);
        self.tasks.push(TaskState {
            id,
            label,
            completed: 0,
            total,
            state,
            transient: options.transient,
            columns: options.columns,
            message: None,
        });
        Ok(id)
    }

    /// Return a clone of one current task fact.
    pub fn task(&self, id: TaskId) -> Option<ProgressFact> {
        self.tasks.iter().find(|task| task.id == id).map(TaskState::fact)
    }

    /// Return all facts in stable insertion order.
    pub fn facts(&self) -> Vec<ProgressFact> {
        self.tasks.iter().map(TaskState::fact).collect()
    }

    /// Return the structured machine-safe task snapshot.
    pub fn snapshot(&self) -> ProgressSnapshot {
        ProgressSnapshot {
            facts: self.facts(),
        }
    }

    /// Return machine facts only when this profile selected machine mode.
    pub fn machine_facts(&self) -> Option<ProgressSnapshot> {
        self.profile.machine_enabled().then(|| self.snapshot())
    }

    /// Apply one typed update through the one state-transition path.
    pub fn apply(&mut self, id: TaskId, update: ProgressUpdate) -> Result<(), ProgressError> {
        match update {
            ProgressUpdate::Increment(amount) => self.increment(id, amount),
            ProgressUpdate::SetPosition(position) => self.set_position(id, position),
            ProgressUpdate::SetTotal(total) => self.set_total(id, total),
            ProgressUpdate::SetColumn(column) => self.set_column(id, column),
            ProgressUpdate::Finish => self.finish(id),
            ProgressUpdate::Fail { message } => self.fail_with_message(id, message),
            ProgressUpdate::Cancel { message } => self.cancel_with_message(id, message),
        }
    }

    /// Add completed units, clamped to a known total.
    pub fn increment(&mut self, id: TaskId, amount: u64) -> Result<(), ProgressError> {
        let task = self.open_task_mut(id)?;
        task.completed = task.completed.saturating_add(amount);
        if let ProgressTotal::Known(total) = task.total {
            task.completed = task.completed.min(total);
        }
        complete_known_task(task);
        Ok(())
    }

    /// Short alias for [`Self::increment`].
    pub fn inc(&mut self, id: TaskId, amount: u64) -> Result<(), ProgressError> {
        self.increment(id, amount)
    }

    pub fn set_position(&mut self, id: TaskId, position: u64) -> Result<(), ProgressError> {
        let task = self.open_task_mut(id)?;
        task.completed = match task.total {
            ProgressTotal::Known(total) => position.min(total),
            ProgressTotal::Unknown => position,
        };
        complete_known_task(task);
        Ok(())
    }

    pub fn set_total(&mut self, id: TaskId, total: Option<u64>) -> Result<(), ProgressError> {
        let task = self.open_task_mut(id)?;
        task.total = total.into();
        if let ProgressTotal::Known(total) = task.total {
            task.completed = task.completed.min(total);
        }
        complete_known_task(task);
        Ok(())
    }

    /// Change whether terminal completion is transient in human frames.
    pub fn set_transient(&mut self, id: TaskId, transient: bool) -> Result<(), ProgressError> {
        let task = self.task_mut(id)?;
        task.transient = transient;
        Ok(())
    }

    /// Insert or replace a custom column while preserving insertion order.
    pub fn set_column(
        &mut self,
        id: TaskId,
        column: ProgressColumn,
    ) -> Result<(), ProgressError> {
        validate_column(&column)?;
        let task = self.open_task_mut(id)?;
        if let Some(existing) = task.columns.iter_mut().find(|item| item.name == column.name) {
            existing.value = column.value;
        } else {
            if task.columns.len() >= MAX_PROGRESS_COLUMNS {
                return Err(ProgressError::Limit {
                    what: "progress columns",
                    limit: MAX_PROGRESS_COLUMNS,
                });
            }
            task.columns.push(column);
        }
        Ok(())
    }

    /// Set a custom column from name/value strings.
    pub fn set_custom_column(
        &mut self,
        id: TaskId,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<(), ProgressError> {
        self.set_column(id, ProgressColumn::new(name, value))
    }

    /// Explicitly finish a task.  Known totals move to their total.
    pub fn finish(&mut self, id: TaskId) -> Result<(), ProgressError> {
        let task = self.open_task_mut(id)?;
        if let ProgressTotal::Known(total) = task.total {
            task.completed = total;
        }
        task.state = ProgressState::Finished;
        task.message = None;
        Ok(())
    }

    /// Explicitly fail a task without a message.
    pub fn fail(&mut self, id: TaskId) -> Result<(), ProgressError> {
        self.fail_with_message(id, None)
    }

    /// Explicitly fail a task with a bounded message.
    pub fn fail_with_message(
        &mut self,
        id: TaskId,
        message: Option<String>,
    ) -> Result<(), ProgressError> {
        validate_message(message.as_deref())?;
        let task = self.open_task_mut(id)?;
        task.state = ProgressState::Failed;
        task.message = message;
        Ok(())
    }

    /// Explicitly cancel a task without a message.
    pub fn cancel(&mut self, id: TaskId) -> Result<(), ProgressError> {
        self.cancel_with_message(id, None)
    }

    /// Explicitly cancel a task with a bounded message.
    pub fn cancel_with_message(
        &mut self,
        id: TaskId,
        message: Option<String>,
    ) -> Result<(), ProgressError> {
        validate_message(message.as_deref())?;
        let task = self.open_task_mut(id)?;
        task.state = ProgressState::Cancelled;
        task.message = message;
        Ok(())
    }

    /// Wrap an existing iterator.  Pulling one item increments the task by one;
    /// exhaustion explicitly finishes it.  Early drop leaves the task running
    /// so callers can choose finish, fail, or cancel explicitly.
    pub fn iter<I>(
        &mut self,
        id: TaskId,
        iterator: I,
    ) -> Result<ProgressIter<'_, 'profile, I>, ProgressError>
    where
        I: Iterator,
    {
        self.ensure_open(id)?;
        Ok(ProgressIter {
            progress: self,
            id,
            iterator,
            exhausted: false,
        })
    }

    /// Add and wrap one iterator in a single operation.
    pub fn track<I>(
        &mut self,
        label: impl Into<String>,
        total: Option<u64>,
        iterator: I,
    ) -> Result<ProgressIter<'_, 'profile, I>, ProgressError>
    where
        I: Iterator,
    {
        let id = self.add_task(label, total)?;
        self.iter(id, iterator)
    }

    /// Render the current human frame, or `None` when the borrowed profile
    /// suppresses progress for a redirected, non-TTY, or machine channel.
    pub fn frame(&self) -> Result<Option<ProgressFrame>, ProgressError> {
        if !self.progress_enabled() {
            return Ok(None);
        }
        self.render_visible_frame().map(Some)
    }

    /// Short alias for [`Self::frame`].
    pub fn render(&self) -> Result<Option<ProgressFrame>, ProgressError> {
        self.frame()
    }

    /// Render only the frame text, without changing the stored diff cursor.
    pub fn frame_text(&self) -> Result<Option<String>, ProgressError> {
        self.frame().map(|frame| frame.map(|frame| frame.text))
    }

    /// Observe facts and, when enabled, advance the deterministic frame diff.
    pub fn observe(&mut self) -> Result<ProgressObservation, ProgressError> {
        let facts = self.facts();
        if !self.progress_enabled() {
            self.previous_frame = None;
            return Ok(ProgressObservation {
                facts,
                frame: None,
                diff: ProgressFrameDiff::empty(),
            });
        }
        let frame = self.render_visible_frame()?;
        let diff = diff_frames(self.previous_frame.as_ref(), Some(&frame));
        self.previous_frame = Some(frame.clone());
        Ok(ProgressObservation {
            facts,
            frame: Some(frame),
            diff,
        })
    }

    /// Forget the previous frame used by [`Self::observe`].
    pub fn reset_diff(&mut self) {
        self.previous_frame = None;
    }

    fn task_mut(&mut self, id: TaskId) -> Result<&mut TaskState, ProgressError> {
        self.tasks
            .iter_mut()
            .find(|task| task.id == id)
            .ok_or(ProgressError::UnknownTask(id))
    }

    fn open_task_mut(&mut self, id: TaskId) -> Result<&mut TaskState, ProgressError> {
        let task = self.task_mut(id)?;
        if task.state.is_terminal() {
            return Err(ProgressError::TaskClosed(id));
        }
        Ok(task)
    }

    fn ensure_open(&self, id: TaskId) -> Result<(), ProgressError> {
        let task = self
            .tasks
            .iter()
            .find(|task| task.id == id)
            .ok_or(ProgressError::UnknownTask(id))?;
        if task.state.is_terminal() {
            return Err(ProgressError::TaskClosed(id));
        }
        Ok(())
    }

    fn render_visible_frame(&self) -> Result<ProgressFrame, ProgressError> {
        let width = self.profile.width().clamp(1, MAX_PROGRESS_WIDTH);
        let unicode = self.profile.unicode_enabled();
        let ansi = self.profile.ansi_enabled();
        let mut lines = Vec::new();
        for task in &self.tasks {
            if task.transient && task.state.is_terminal() {
                continue;
            }
            let marker = marker(task.state, unicode);
            let plain = render_task_line(task, marker);
            let clipped = clip_line(&plain, width, unicode);
            lines.push(style_marker(&clipped, marker, ansi, task.state));
        }
        ProgressFrame::from_lines(lines)
    }
}

/// Structured facts returned by machine consumers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgressSnapshot {
    /// All tasks in insertion order, including hidden transient tasks.
    pub facts: Vec<ProgressFact>,
}

impl ProgressSnapshot {
    /// Return the number of facts.
    pub fn len(&self) -> usize {
        self.facts.len()
    }

    /// Return true when no task exists.
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }

    /// Find one fact by stable task identity.
    pub fn task(&self, id: TaskId) -> Option<&ProgressFact> {
        self.facts.iter().find(|fact| fact.id == id)
    }
}

/// Iterator adapter that updates one task as values are pulled.
pub struct ProgressIter<'progress, 'profile, I>
where
    I: Iterator,
{
    progress: &'progress mut MultiProgress<'profile>,
    id: TaskId,
    iterator: I,
    exhausted: bool,
}

impl<'progress, 'profile, I> ProgressIter<'progress, 'profile, I>
where
    I: Iterator,
{
    /// Return the task being updated.
    pub const fn task_id(&self) -> TaskId {
        self.id
    }

    /// Return the wrapped iterator without changing task state.
    pub fn into_inner(self) -> I {
        self.iterator
    }
}

impl<'progress, 'profile, I> Iterator for ProgressIter<'progress, 'profile, I>
where
    I: Iterator,
{
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        match self.iterator.next() {
            Some(item) => {
                let _ = self.progress.increment(self.id, 1);
                Some(item)
            }
            None => {
                if !self.exhausted {
                    let _ = self.progress.finish(self.id);
                    self.exhausted = true;
                }
                None
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iterator.size_hint()
    }
}

fn validate_label(label: &str) -> Result<(), ProgressError> {
    validate_text("progress task label", label, MAX_PROGRESS_LABEL_BYTES, true)
}

fn validate_message(message: Option<&str>) -> Result<(), ProgressError> {
    if let Some(message) = message {
        validate_text("progress task message", message, MAX_PROGRESS_MESSAGE_BYTES, false)?;
    }
    Ok(())
}

fn validate_column(column: &ProgressColumn) -> Result<(), ProgressError> {
    validate_text(
        "progress column name",
        &column.name,
        MAX_PROGRESS_COLUMN_NAME_BYTES,
        true,
    )?;
    validate_text(
        "progress column value",
        &column.value,
        MAX_PROGRESS_COLUMN_VALUE_BYTES,
        false,
    )
}

fn validate_columns(columns: &[ProgressColumn]) -> Result<(), ProgressError> {
    if columns.len() > MAX_PROGRESS_COLUMNS {
        return Err(ProgressError::Limit {
            what: "progress columns",
            limit: MAX_PROGRESS_COLUMNS,
        });
    }
    for column in columns {
        validate_column(column)?;
    }
    Ok(())
}

fn validate_text(
    what: &'static str,
    value: &str,
    limit: usize,
    empty_is_invalid: bool,
) -> Result<(), ProgressError> {
    if value.len() > limit {
        return Err(ProgressError::Limit { what, limit });
    }
    if (empty_is_invalid && value.is_empty()) || value.chars().any(char::is_control) {
        return Err(ProgressError::InvalidText { what });
    }
    Ok(())
}

fn marker(state: ProgressState, unicode: bool) -> &'static str {
    match (state, unicode) {
        (ProgressState::Running, true) => "…",
        (ProgressState::Running, false) => ">",
        (ProgressState::Finished, true) => "✓",
        (ProgressState::Finished, false) => "done",
        (ProgressState::Failed, true) => "✗",
        (ProgressState::Failed, false) => "fail",
        (ProgressState::Cancelled, true) => "×",
        (ProgressState::Cancelled, false) => "cancel",
    }
}

fn render_task_line(task: &TaskState, marker: &str) -> String {
    let progress = match task.total {
        ProgressTotal::Known(total) => format!("{}/{}", task.completed, total),
        ProgressTotal::Unknown => format!("{}/?", task.completed),
    };
    let mut line = format!("{marker} {} {progress} {}", task.label, task.state.as_str());
    for column in &task.columns {
        line.push_str("  ");
        line.push_str(&column.name);
        line.push('=');
        line.push_str(&column.value);
    }
    if let Some(message) = &task.message {
        line.push_str("  ");
        line.push_str(message);
    }
    line
}

fn clip_line(value: &str, width: usize, unicode: bool) -> String {
    crate::AdaptiveTable::clip_to_width_with_unicode(value, width, unicode)
}

fn style_marker(value: &str, marker: &str, ansi: bool, state: ProgressState) -> String {
    if !ansi || !value.starts_with(marker) {
        return value.to_string();
    }
    let color = match state {
        ProgressState::Running => "36",
        ProgressState::Finished => "32",
        ProgressState::Failed => "31",
        ProgressState::Cancelled => "33",
    };
    format!("\x1b[{color}m{marker}\x1b[0m{}", &value[marker.len()..])
}
