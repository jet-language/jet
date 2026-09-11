//! Deterministic, in-process terminal driving for TUI tests.
//!
//! `HeadlessDriver` owns terminal size and sends semantic input events to the
//! same reducer used by the real interactive surface. It never enters raw
//! mode, opens a PTY, reads process streams, or emits terminal control bytes.
//! The returned `Frame` is therefore safe to compare directly in a test or to
//! pass to a separate recording/snapshot layer.
use crate::Help::Interactive::State;
use crate::Recording::{Recording, RecordingError, RecordingEvent, RecordingIdentity};
use crate::Tape::{normalize_terminal_text, Tape, TapeError, TapeExecutor, TapeRun, TapeStep};
use std::collections::BTreeMap;
use std::time::Duration;
pub use crate::Term::Key;

/// A terminal viewport used by a headless target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Size {
    pub width: usize,
    pub height: usize,
}

impl Size {
    pub const fn new(width: usize, height: usize) -> Self {
        Self { width, height }
    }

    pub const fn columns(self) -> usize {
        self.width
    }

    pub const fn rows(self) -> usize {
        self.height
    }
}

/// A point in the viewport. Coordinates are zero-based.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    pub column: usize,
    pub row: usize,
}

impl Cursor {
    pub const fn new(column: usize, row: usize) -> Self {
        Self { column, row }
    }

    pub const fn x(self) -> usize {
        self.column
    }

    pub const fn y(self) -> usize {
        self.row
    }
}

/// Semantic input accepted by a headless TUI target.
#[derive(Clone, Debug, PartialEq)]
pub enum InputEvent {
    Key(Key),
    Click { column: usize, row: usize },
    Resize(Size),
    /// Let the target process work that is already pending without sleeping.
    Wait,
}

impl InputEvent {
    pub const fn key(key: Key) -> Self {
        Self::Key(key)
    }

    pub const fn click(column: usize, row: usize) -> Self {
        Self::Click { column, row }
    }

    pub const fn resize(width: usize, height: usize) -> Self {
        Self::Resize(Size::new(width, height))
    }

    pub const fn wait() -> Self {
        Self::Wait
    }
}

/// A stable frame captured from a headless target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub size: Size,
    pub text: String,
    /// The semantic target that currently owns keyboard focus, when one is
    /// available. A category, command, or diagnostic code is reported by its
    /// canonical display name.
    pub focus: Option<String>,
    /// The selected target's zero-based viewport position, when it is visible.
    pub cursor: Option<Cursor>,
}

impl Frame {
    pub fn content(&self) -> &str {
        &self.text
    }

    pub fn lines(&self) -> std::str::Lines<'_> {
        self.text.lines()
    }

    pub fn line(&self, row: usize) -> Option<&str> {
        self.text.lines().nth(row)
    }
}

/// The small seam a headless driver needs from a real TUI.
///
/// A target owns semantic state and rendering. The driver owns only viewport
/// size and event transport, so adding capture cannot create a second widget
/// tree or a second renderer.
pub trait RenderTarget {
    fn render(&self) -> String;
    fn send(&mut self, event: InputEvent);
    fn resize(&mut self, size: Size);
    fn wait(&mut self);
    fn focus(&self) -> Option<String>;
    fn cursor(&self) -> Option<Cursor>;
    /// Report a target-owned readiness marker without inspecting rendered text.
    ///
    /// Custom targets can expose domain-specific readiness.  The conservative
    /// default keeps old targets safe: an unimplemented marker never passes.
    fn ready(&self, _marker: &str) -> bool {
        false
    }
}

/// In-process terminal driver. The default target is Jet's real interactive
/// command palette reducer; custom real TUI targets may be supplied with
/// [`HeadlessDriver::with_target`].
pub struct HeadlessDriver {
    size: Size,
    target: Box<dyn RenderTarget>,
}

impl HeadlessDriver {
    /// Create a no-color driver over the real interactive TUI.
    pub fn new(size: Size) -> Self {
        Self::with_target(size, StateTarget::new(size, false))
    }

    /// Create a driver over the real interactive TUI with explicit color.
    pub fn with_color(size: Size, color: bool) -> Self {
        Self::with_target(size, StateTarget::new(size, color))
    }

    /// Create a driver around another real TUI target that implements the
    /// shared reducer/render seam.
    pub fn with_target<T>(size: Size, target: T) -> Self
    where
        T: RenderTarget + 'static,
    {
        let size = normalized_size(size);
        let mut driver = Self {
            size,
            target: Box::new(target),
        };
        driver.target.resize(size);
        driver
    }

    pub fn size(&self) -> Size {
        self.size
    }

    /// Capture the current semantic frame without terminal I/O.
    pub fn render(&self) -> Frame {
        Frame {
            size: self.size,
            text: self.target.render(),
            focus: self.target.focus(),
            cursor: self.target.cursor(),
        }
    }

    /// Append the current frame to a deterministic recording.
    pub fn record_frame(&self, recording: &mut Recording) -> Result<usize, RecordingError> {
        recording.push_frame(&self.render())
    }

    /// Send one semantic event to the target.
    pub fn send(&mut self, event: InputEvent) {
        match event {
            InputEvent::Resize(size) => self.resize(size),
            InputEvent::Wait => self.wait(),
            event => self.target.send(event),
        }
    }

    pub fn press(&mut self, key: Key) {
        self.send(InputEvent::Key(key));
    }

    pub fn click(&mut self, column: usize, row: usize) {
        self.send(InputEvent::click(column, row));
    }

    pub fn resize(&mut self, size: Size) {
        let size = normalized_size(size);
        self.size = size;
        self.target.resize(size);
    }

    /// Process pending target work once. The operation never sleeps, making a
    /// test's clock and output independent of host scheduling.
    pub fn wait(&mut self) {
        self.target.wait();
    }

    /// Query target-owned readiness state without parsing the rendered frame.
    pub fn ready(&self, marker: &str) -> bool {
        self.target.ready(marker)
    }
    pub fn focus(&self) -> Option<String> {
        self.target.focus()
    }

    pub fn cursor(&self) -> Option<Cursor> {
        self.target.cursor()
    }
}
/// A deterministic headless interaction session with frame capture.
///
/// The session is both a [`TapeExecutor`] and a recording owner. Every input,
/// virtual-clock operation, readiness check, environment override, and output
/// observation is appended to the recording together with the resulting frame.
pub struct HeadlessReplay {
    driver: HeadlessDriver,
    recording: Recording,
    environment: BTreeMap<String, String>,
    elapsed: Duration,
}

impl HeadlessReplay {
    /// Create a replay session whose viewport and color policy come from its
    /// explicit recording identity.
    pub fn new(identity: RecordingIdentity) -> Result<Self, RecordingError> {
        let size = Size::new(usize::from(identity.width), usize::from(identity.height));
        let color = identity.color;
        Self::with_driver(identity, HeadlessDriver::with_color(size, color))
    }

    /// Wrap a custom target while retaining the recording identity contract.
    pub fn with_driver(
        identity: RecordingIdentity,
        driver: HeadlessDriver,
    ) -> Result<Self, RecordingError> {
        let size = driver.size();
        if usize::from(identity.width) != size.width
            || usize::from(identity.height) != size.height
        {
            return Err(RecordingError::Invalid(
                "headless driver dimensions do not match recording identity".into(),
            ));
        }
        let mut recording = Recording::new(identity)?;
        driver.record_frame(&mut recording)?;
        Ok(Self {
            driver,
            recording,
            environment: BTreeMap::new(),
            elapsed: Duration::ZERO,
        })
    }

    /// Replay a parsed tape through the shared headless reducer.
    pub fn replay(&mut self, tape: &Tape) -> Result<TapeRun, TapeError> {
        tape.replay(self)
    }

    /// Parse and replay one canonical tape source.
    pub fn replay_source(&mut self, source: &str) -> Result<TapeRun, TapeError> {
        Tape::replay_source(source, self)
    }

    /// Return the current driver.
    pub fn driver(&self) -> &HeadlessDriver {
        &self.driver
    }

    /// Return mutable access to the current driver for custom setup.
    pub fn driver_mut(&mut self) -> &mut HeadlessDriver {
        &mut self.driver
    }

    /// Return the recording, including the initial and subsequent frames.
    pub fn recording(&self) -> &Recording {
        &self.recording
    }

    /// Return mutable access to the recording for explicit annotations.
    pub fn recording_mut(&mut self) -> &mut Recording {
        &mut self.recording
    }

    /// Return explicit environment overrides without touching the process.
    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }

    /// Return virtual elapsed time accumulated by `Sleep` steps.
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// Consume the session and return its driver and recording.
    pub fn into_parts(self) -> (HeadlessDriver, Recording) {
        (self.driver, self.recording)
    }

    /// Capture one normalized output observation and its resulting frame.
    pub fn capture_output(&mut self) -> Result<String, RecordingError> {
        let output = normalize_terminal_text(&self.driver.render().text);
        self.record_action(RecordingEvent::Output(output.clone()))
            .map_err(RecordingError::Event)?;
        Ok(output)
    }


    fn record_action(&mut self, event: RecordingEvent) -> Result<(), String> {
        self.recording
            .push(event)
            .map_err(|error| error.to_string())?;
        self.driver
            .record_frame(&mut self.recording)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

impl TapeExecutor for HeadlessReplay {
    fn type_text(&mut self, text: &str) -> Result<(), String> {
        for character in text.chars() {
            self.driver.press(Key::Char(character));
        }
        self.record_action(RecordingEvent::Type(text.to_string()))
    }

    fn enter(&mut self) -> Result<(), String> {
        self.driver.press(Key::Enter);
        self.record_action(RecordingEvent::Enter)
    }

    fn sleep(&mut self, duration: Duration) -> Result<(), String> {
        self.elapsed = self.elapsed.saturating_add(duration);
        self.driver.wait();
        self.record_action(RecordingEvent::Sleep(duration.as_millis() as u64))
    }

    fn wait(&mut self, readiness: &str) -> Result<(), String> {
        self.driver.wait();
        if !self.driver.ready(readiness) {
            return Err(format!("readiness marker `{readiness}` was not observed"));
        }
        self.record_action(RecordingEvent::Wait(readiness.to_string()))
    }

    fn require(&mut self, program: &str) -> Result<(), String> {
        Tape::new(vec![TapeStep::Require(program.to_string())])
            .and_then(|tape| tape.preflight())
            .map_err(|error| error.to_string())?;
        self.record_action(RecordingEvent::Require(program.to_string()))
    }

    fn set(&mut self, name: &str, value: &str) -> Result<(), String> {
        Tape::new(vec![TapeStep::Set {
            name: name.to_string(),
            value: value.to_string(),
        }])
        .map_err(|error| error.to_string())?;
        self.record_action(RecordingEvent::Set {
            name: name.to_string(),
            value: value.to_string(),
        })?;
        self.environment.insert(name.to_string(), value.to_string());
        Ok(())
    }

    fn observe_output(&mut self) -> Result<String, String> {
        let output = normalize_terminal_text(&self.driver.render().text);
        self.record_action(RecordingEvent::Output(output.clone()))?;
        Ok(output)
    }
}


fn normalized_size(size: Size) -> Size {
    Size::new(size.width.max(1), size.height.max(1))
}

struct StateTarget {
    state: State,
}

impl StateTarget {
    fn new(size: Size, color: bool) -> Self {
        Self {
            state: State::new(size.width, size.height, color),
        }
    }
}

impl RenderTarget for StateTarget {
    fn render(&self) -> String {
        self.state.render()
    }

    fn send(&mut self, event: InputEvent) {
        match event {
            InputEvent::Key(key) => {
                let _ = self.state.apply_key(key);
            }
            InputEvent::Click { column, row } => {
                let _ = self.state.click(column, row);
            }
            InputEvent::Resize(size) => self.resize(size),
            InputEvent::Wait => self.wait(),
        }
    }

    fn resize(&mut self, size: Size) {
        self.state.resize(size.width, size.height);
    }

    fn wait(&mut self) {
        self.state.wait();
    }

    fn focus(&self) -> Option<String> {
        self.state.focus_name()
    }

    fn cursor(&self) -> Option<Cursor> {
        self.state.cursor().map(|(column, row)| Cursor::new(column, row))
    }
    fn ready(&self, marker: &str) -> bool {
        self.state.ready(marker)
    }
}
