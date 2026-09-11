// Dependency-free game development swap facts shared by hosts and generated code.
//
// The host owns filesystem/build/world facts and supplies them here. This
// module validates the declared operation, records one typed outcome, and
// never infers game meaning from a path or payload.

/// One canonical frame-count plan shared by every game host.
///
/// A live backend has no bound (`None`); a headless backend starts with the
/// historical three-frame default.  Hosts only execute the plan: they do not
/// choose a count, validate a request, or advance the cursor independently.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameFrameBudget {
    remaining: Option<i64>,
    next_index: i64,
    frame_in_flight: bool,
}

impl Default for JetGameFrameBudget {
    fn default() -> Self {
        Self::default_headless()
    }
}

impl JetGameFrameBudget {
    pub const DEFAULT_HEADLESS_FRAMES: i64 = 3;
    pub const FRAME_BUDGET_ERROR: &'static str = "`game.run` frame budget must be positive";

    pub const fn live() -> Self {
        Self {
            remaining: None,
            next_index: 0,
            frame_in_flight: false,
        }
    }

    pub const fn default_headless() -> Self {
        Self {
            remaining: Some(Self::DEFAULT_HEADLESS_FRAMES),
            next_index: 0,
            frame_in_flight: false,
        }
    }

    pub const fn validate(frames: i64) -> Result<(), &'static str> {
        if frames > 0 {
            Ok(())
        } else {
            Err(Self::FRAME_BUDGET_ERROR)
        }
    }

    pub fn with_frame_count(frames: i64) -> Result<Self, &'static str> {
        Self::validate(frames)?;
        Ok(Self {
            remaining: Some(frames),
            next_index: 0,
            frame_in_flight: false,
        })
    }

    pub fn set_requested(&mut self, frames: Option<i64>) -> Result<(), &'static str> {
        if let Some(frames) = frames {
            *self = Self::with_frame_count(frames)?;
        }
        Ok(())
    }

    pub fn should_continue(&self) -> bool {
        !self.frame_in_flight && self.remaining.map_or(true, |remaining| remaining > 0)
    }

    pub fn next_frame(&mut self) -> Option<i64> {
        if !self.should_continue() {
            return None;
        }
        self.frame_in_flight = true;
        let index = self.next_index;
        self.next_index = self.next_index.saturating_add(1);
        Some(index)
    }

    pub fn present(&mut self) {
        self.frame_in_flight = false;
        if let Some(remaining) = self.remaining.as_mut() {
            *remaining = remaining.saturating_sub(1);
        }
    }
}

/// Canonical wire hash for deterministic frame transcripts.
#[derive(Clone, Debug)]
pub struct JetGameTranscriptHasher {
    state: u64,
}

impl JetGameTranscriptHasher {
    pub fn new() -> Self {
        let mut hasher = Self {
            state: 0xcbf29ce484222325,
        };
        hasher.push_bytes(b"jet.game.transcript.v1\0");
        hasher
    }

    fn push_bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.state ^= u64::from(*byte);
            self.state = self.state.wrapping_mul(0x100000001b3);
        }
    }

    fn push_u64(&mut self, value: u64) {
        self.push_bytes(&value.to_le_bytes());
    }

    pub fn push_frame(&mut self, index: i64, input: &[String]) {
        self.push_bytes(b"frame\0");
        self.push_u64(index as u64);
        self.push_u64(input.len() as u64);
        for action in input {
            self.push_u64(action.len() as u64);
            self.push_bytes(action.as_bytes());
        }
    }

    pub fn finish(self) -> String {
        format!("{:016x}", self.state)
    }
}

/// Canonical text replay format shared by native, JIT, and host adapters.
/// The first non-empty line may be `jet.game.replay.v1`; every other line is
/// `<frame-index>\t<action>[,<action>...]`. Empty actions use `none`.
pub const JET_GAME_REPLAY_MAX_BYTES: usize = 2 * 1024 * 1024;
pub const JET_GAME_REPLAY_MAX_FRAMES: usize = 65_536;
pub const JET_GAME_REPLAY_MAX_ACTIONS_PER_FRAME: usize = 256;
pub const JET_GAME_REPLAY_MAX_ACTION_BYTES: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameReplayFrame {
    pub index: i64,
    pub actions: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameReplay {
    frames: Vec<JetGameReplayFrame>,
}

impl JetGameReplay {
    pub fn from_path(path: &str) -> Result<Self, String> {
        if path.trim().is_empty() {
            return Err("game replay path must not be empty".to_string());
        }
        let bytes = std::fs::read(path)
            .map_err(|error| format!("could not read game replay `{path}`: {error}"))?;
        if bytes.len() > JET_GAME_REPLAY_MAX_BYTES {
            return Err("game replay exceeds the retained byte limit".to_string());
        }
        let source = String::from_utf8(bytes)
            .map_err(|_| "game replay must be UTF-8 text".to_string())?;
        let mut frames = Vec::new();
        let mut previous_index = None;
        for (line_number, raw_line) in source.lines().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() || (frames.is_empty() && line == "jet.game.replay.v1") {
                continue;
            }
            let (raw_index, raw_actions) = line.split_once('\t').ok_or_else(|| {
                format!("game replay line {} requires a tab separator", line_number + 1)
            })?;
            let index = raw_index.parse::<i64>().map_err(|_| {
                format!("game replay line {} has an invalid frame index", line_number + 1)
            })?;
            if index < 0 || previous_index.is_some_and(|previous| index <= previous) {
                return Err(format!(
                    "game replay line {} frame indexes must be non-negative and increasing",
                    line_number + 1
                ));
            }
            if frames.len() >= JET_GAME_REPLAY_MAX_FRAMES {
                return Err("game replay exceeds the retained frame limit".to_string());
            }
            let actions = if raw_actions == "none" || raw_actions.is_empty() {
                Vec::new()
            } else {
                let mut actions = Vec::new();
                for action in raw_actions.split(',') {
                    if actions.len() >= JET_GAME_REPLAY_MAX_ACTIONS_PER_FRAME {
                        return Err(format!(
                            "game replay line {} exceeds the action limit",
                            line_number + 1
                        ));
                    }
                    if action.is_empty()
                        || action.len() > JET_GAME_REPLAY_MAX_ACTION_BYTES
                        || action.chars().any(char::is_control)
                    {
                        return Err(format!(
                            "game replay line {} contains an invalid action",
                            line_number + 1
                        ));
                    }
                    actions.push(action.to_string());
                }
                actions
            };
            frames.push(JetGameReplayFrame { index, actions });
            previous_index = Some(index);
        }
        Ok(Self { frames })
    }

    pub fn actions_at(&self, index: i64) -> Option<&[String]> {
        self.frames
            .binary_search_by_key(&index, |frame| frame.index)
            .ok()
            .map(|position| self.frames[position].actions.as_slice())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum JetGameChangeKind {
    Asset,
    Script,
    World,
}

impl JetGameChangeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Asset => "asset",
            Self::Script => "script",
            Self::World => "world",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum JetGameMigrationDecision {
    Preserve,
    Migrate,
    Reset,
    Reject,
}

impl JetGameMigrationDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Preserve => "preserve",
            Self::Migrate => "migrate",
            Self::Reset => "reset",
            Self::Reject => "reject",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum JetGameSwapStatus {
    Applied,
    Rejected,
}

impl JetGameSwapStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Rejected => "rejected",
        }
    }

    pub const fn is_applied(self) -> bool {
        matches!(self, Self::Applied)
    }
}

/// Checked facts for exactly one game change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameChangeFact {
    pub path: String,
    pub kind: JetGameChangeKind,
    pub old_schema_id: Option<String>,
    pub new_schema_id: Option<String>,
    pub migration: JetGameMigrationDecision,
    pub reason: String,
}

impl JetGameChangeFact {
    pub fn new(
        path: impl Into<String>,
        kind: JetGameChangeKind,
        old_schema_id: Option<String>,
        new_schema_id: Option<String>,
        migration: JetGameMigrationDecision,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        let fact = Self {
            path: path.into(),
            kind,
            old_schema_id,
            new_schema_id,
            migration,
            reason: reason.into(),
        };
        fact.validate()?;
        Ok(fact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.path.trim().is_empty() {
            return Err("game change fact path is empty".to_string());
        }
        if self.reason.trim().is_empty() {
            return Err(format!(
                "game change fact `{}` has no checked reason",
                self.path
            ));
        }
        for (label, value) in [
            ("old schema id", self.old_schema_id.as_deref()),
            ("new schema id", self.new_schema_id.as_deref()),
        ] {
            if value.is_some_and(|value| value.trim().is_empty()) {
                return Err(format!("game change fact `{}` is empty", label));
            }
        }
        if matches!(self.migration, JetGameMigrationDecision::Migrate)
            && (self.old_schema_id.is_none() || self.new_schema_id.is_none())
        {
            return Err(format!(
                "game change fact `{}` cannot migrate without old and new schema ids",
                self.path
            ));
        }
        if matches!(self.migration, JetGameMigrationDecision::Reset) && self.new_schema_id.is_none()
        {
            return Err(format!(
                "game change fact `{}` cannot reset without a new schema id",
                self.path
            ));
        }
        Ok(())
    }
    /// Replace the checked migration verdict without re-deriving path or
    /// schema identity. Used when the surrounding transaction rejects.
    pub fn with_migration(
        self,
        migration: JetGameMigrationDecision,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        Self::new(
            self.path,
            self.kind,
            self.old_schema_id,
            self.new_schema_id,
            migration,
            reason,
        )
    }
}

/// Result returned after the shared kernel accepts or rejects a checked fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameSwapOutcome {
    pub sequence: u64,
    pub fact: JetGameChangeFact,
    pub status: JetGameSwapStatus,
}

impl JetGameSwapOutcome {
    pub const fn is_applied(&self) -> bool {
        self.status.is_applied()
    }

    pub const fn kind(&self) -> JetGameChangeKind {
        self.fact.kind
    }
}

/// One canonical stateful operation kernel for game asset/script/world swaps.
#[derive(Clone, Debug, Default)]
pub struct JetGameSwapKernel {
    next_sequence: u64,
    last_outcome: Option<JetGameSwapOutcome>,
}

impl JetGameSwapKernel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply_asset_swap(
        &mut self,
        fact: JetGameChangeFact,
    ) -> Result<JetGameSwapOutcome, String> {
        self.apply(JetGameChangeKind::Asset, fact)
    }

    pub fn apply_script_reload(
        &mut self,
        fact: JetGameChangeFact,
    ) -> Result<JetGameSwapOutcome, String> {
        self.apply(JetGameChangeKind::Script, fact)
    }

    pub fn apply_world_reload(
        &mut self,
        fact: JetGameChangeFact,
    ) -> Result<JetGameSwapOutcome, String> {
        self.apply(JetGameChangeKind::World, fact)
    }

    pub fn last_outcome(&self) -> Option<&JetGameSwapOutcome> {
        self.last_outcome.as_ref()
    }

    fn apply(
        &mut self,
        expected_kind: JetGameChangeKind,
        fact: JetGameChangeFact,
    ) -> Result<JetGameSwapOutcome, String> {
        fact.validate()?;
        if fact.kind != expected_kind {
            return Err(format!(
                "game swap operation `{}` received `{}` fact",
                expected_kind.as_str(),
                fact.kind.as_str()
            ));
        }
        self.next_sequence = self.next_sequence.saturating_add(1);
        let status = if matches!(fact.migration, JetGameMigrationDecision::Reject) {
            JetGameSwapStatus::Rejected
        } else {
            JetGameSwapStatus::Applied
        };
        let outcome = JetGameSwapOutcome {
            sequence: self.next_sequence,
            fact,
            status,
        };
        self.last_outcome = Some(outcome.clone());
        Ok(outcome)
    }
}

/// Host-facing session wrapper.  Generated Prelude code wraps the same
/// dependency-free kernel instead of implementing a second decision path.
#[derive(Clone, Debug, Default)]
pub struct JetGameDevSession {
    pub swap_kernel: JetGameSwapKernel,
}

impl JetGameDevSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply_asset_swap(
        &mut self,
        fact: JetGameChangeFact,
    ) -> Result<JetGameSwapOutcome, String> {
        self.swap_kernel.apply_asset_swap(fact)
    }

    pub fn apply_script_reload(
        &mut self,
        fact: JetGameChangeFact,
    ) -> Result<JetGameSwapOutcome, String> {
        self.swap_kernel.apply_script_reload(fact)
    }

    pub fn apply_world_reload(
        &mut self,
        fact: JetGameChangeFact,
    ) -> Result<JetGameSwapOutcome, String> {
        self.swap_kernel.apply_world_reload(fact)
    }

    pub fn last_outcome(&self) -> Option<&JetGameSwapOutcome> {
        self.swap_kernel.last_outcome()
    }
}
