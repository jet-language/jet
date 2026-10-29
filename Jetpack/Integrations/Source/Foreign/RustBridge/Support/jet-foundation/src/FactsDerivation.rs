//! Derivation records shared by evidence reports and generated AOT Prelude.
//!
//! Kept independent of the compiler fact registry so native programs can
//! splice this source without pulling TargetMachine, AST, or Policy.

/// Evidence method retained by a checked derivation record.
///
/// These methods are deliberately not ordered by confidence.  A renderer must
/// preserve the producer's method and may not promote a sampled or observed
/// result into a proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DerivationMethod {
    StaticDerivation,
    FormalProof,
    RecordedExecution,
    SampledAgreement,
    ExternalAssumption,
}

impl DerivationMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StaticDerivation => "static_derivation",
            Self::FormalProof => "formal_proof",
            Self::RecordedExecution => "recorded_execution",
            Self::SampledAgreement => "sampled_agreement",
            Self::ExternalAssumption => "external_assumption",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "static_derivation" => Some(Self::StaticDerivation),
            "formal_proof" => Some(Self::FormalProof),
            "recorded_execution" => Some(Self::RecordedExecution),
            "sampled_agreement" => Some(Self::SampledAgreement),
            "external_assumption" => Some(Self::ExternalAssumption),
            _ => None,
        }
    }
}

/// Current applicability of one retained derivation.
///
/// `Unknown` is used when the producer did not establish a current value.  It
/// is intentionally separate from `Unavailable` and never implies success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DerivationDisposition {
    Current,
    Stale,
    Expired,
    Redacted,
    Unavailable,
    Unsupported,
    BudgetExhausted,
    Unknown,
}

impl DerivationDisposition {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Stale => "stale",
            Self::Expired => "expired",
            Self::Redacted => "redacted",
            Self::Unavailable => "unavailable",
            Self::Unsupported => "unsupported",
            Self::BudgetExhausted => "budget_exhausted",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "current" => Some(Self::Current),
            "stale" => Some(Self::Stale),
            "expired" => Some(Self::Expired),
            "redacted" => Some(Self::Redacted),
            "unavailable" => Some(Self::Unavailable),
            "unsupported" => Some(Self::Unsupported),
            "budget_exhausted" => Some(Self::BudgetExhausted),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }

    pub const fn is_current(self) -> bool {
        matches!(self, Self::Current)
    }

    pub const fn is_unknown(self) -> bool {
        matches!(self, Self::Unknown)
    }
}

/// Optional observation attached to a checked derivation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DerivationObservation {
    pub event_id: Option<String>,
    pub counterexample_id: Option<String>,
}

impl DerivationObservation {
    pub fn event(event_id: impl Into<String>) -> Self {
        Self {
            event_id: Some(event_id.into()),
            counterexample_id: None,
        }
    }

    pub fn counterexample(counterexample_id: impl Into<String>) -> Self {
        Self {
            event_id: None,
            counterexample_id: Some(counterexample_id.into()),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.event_id.is_none() && self.counterexample_id.is_none()
    }
}

/// Identity of the inputs that established one checked claim.
///
/// Empty components are meaningful: static derivations normally have no run,
/// while recorded executions must fill it. Consumers compare all four
/// components and mark dependents stale instead of guessing a replacement.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DerivationIdentity {
    pub source: String,
    pub build: String,
    pub run: String,
    pub target: String,
}

/// Typed producer payload carried by a canonical derivation. The relation is
/// extensible by adding another checked payload variant; consumers must not
/// deserialize an untyped blob or derive a replacement graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DerivationPayload {
    FrameSchedule(crate::ResourceSchedule::JetFrameSchedule),
}

impl DerivationPayload {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::FrameSchedule(_) => "jet_frame_schedule",
        }
    }

    pub fn frame_schedule(&self) -> Option<&crate::ResourceSchedule::JetFrameSchedule> {
        match self {
            Self::FrameSchedule(schedule) => Some(schedule),
        }
    }

    fn identity(&self) -> String {
        match self {
            Self::FrameSchedule(schedule) => schedule.canonical_json(),
        }
    }
    /// Canonical payload JSON is owned by the typed schedule module. This
    /// wrapper keeps the derivation relation typed without duplicating its
    /// wire format in Facts.
    pub fn to_json(&self) -> String {
        match self {
            Self::FrameSchedule(schedule) => schedule.canonical_json(),
        }
    }


}

impl DerivationIdentity {
    pub fn new(
        source: impl Into<String>,
        build: impl Into<String>,
        run: impl Into<String>,
        target: impl Into<String>,
    ) -> Self {
        Self {
            source: source.into(),
            build: build.into(),
            run: run.into(),
            target: target.into(),
        }
    }

    pub fn differs_from(&self, other: &Self) -> bool {
        self != other
    }

    pub fn is_empty(&self) -> bool {
        self.source.is_empty()
            && self.build.is_empty()
            && self.run.is_empty()
            && self.target.is_empty()
    }
}

/// Compact reason relation retained beside an existing evidence record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivationRecord {
    pub id: String,
    pub subject: String,
    pub claim: String,
    pub producer: String,
    pub method: DerivationMethod,
    pub rule: String,
    pub premises: Vec<String>,
    pub identity: DerivationIdentity,
    pub assumptions: Vec<String>,
    pub disposition: DerivationDisposition,
    pub observation: Option<DerivationObservation>,
    /// Typed producer payload, interned with this canonical relation.
    pub payload: Option<DerivationPayload>,
}

impl DerivationRecord {
    pub fn new(
        subject: impl Into<String>,
        claim: impl Into<String>,
        producer: impl Into<String>,
        method: DerivationMethod,
        rule: impl Into<String>,
        premises: impl IntoIterator<Item = String>,
        identity: DerivationIdentity,
    ) -> Self {
        let subject = subject.into();
        let claim = claim.into();
        let producer = producer.into();
        let rule = rule.into();
        let mut premises = premises.into_iter().collect::<Vec<_>>();
        premises.sort();
        premises.dedup();
        let mut record = Self {
            id: String::new(),
            subject,
            claim,
            producer,
            method,
            rule,
            premises,
            identity,
            assumptions: Vec::new(),
            disposition: DerivationDisposition::Current,
            observation: None,
            payload: None,
        };
        record.id = record.canonical_id();
        record
    }

    pub fn with_assumptions(mut self, assumptions: impl IntoIterator<Item = String>) -> Self {
        self.assumptions = assumptions.into_iter().collect();
        self.assumptions.sort();
        self.assumptions.dedup();
        self
    }

    pub fn with_disposition(mut self, disposition: DerivationDisposition) -> Self {
        self.disposition = disposition;
        self
    }

    pub fn with_observation(mut self, observation: DerivationObservation) -> Self {
        self.observation = (!observation.is_empty()).then_some(observation);
        self
    }

    /// Mark a retained reason stale when any source/build/run/target input
    /// changes. No other disposition is silently promoted back to current.
    pub fn invalidate_if_identity_changed(&mut self, current: &DerivationIdentity) -> bool {
        if self.disposition == DerivationDisposition::Current
            && self.identity.differs_from(current)
        {
            self.disposition = DerivationDisposition::Stale;
            return true;
        }
        false
    }

    fn canonical_id(&self) -> String {
        let payload_identity = self
            .payload
            .as_ref()
            .map(DerivationPayload::identity)
            .unwrap_or_default();
        derivation_token(&[
            &self.subject,
            &self.claim,
            &self.producer,
            self.method.as_str(),
            &self.rule,
            &self.premises.join("\0"),
            &self.identity.source,
            &self.identity.build,
            &self.identity.run,
            &self.identity.target,
            &payload_identity,
        ])
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id != self.canonical_id() {
            return Err(format!("derivation {} has a non-canonical identity", self.id));
        }
        Ok(())
    }

    /// Attach the typed payload selected by the authoritative producer.
    /// Payload identity participates in the interned id, so a changed plan
    /// cannot masquerade as the previous checked relation.
    pub fn with_payload(mut self, payload: DerivationPayload) -> Self {
        self.payload = Some(payload);
        self.id = self.canonical_id();
        self
    }

    pub fn reference(&self) -> DerivationRef {
        DerivationRef {
            id: self.id.clone(),
        }
    }

    pub fn to_json(&self) -> String {
        let premises = self
            .premises
            .iter()
            .map(|value| format!("\"{}\"", json_escape(value)))
            .collect::<Vec<_>>()
            .join(",");
        let assumptions = self
            .assumptions
            .iter()
            .map(|value| format!("\"{}\"", json_escape(value)))
            .collect::<Vec<_>>()
            .join(",");
        let observation = self.observation.as_ref().map_or_else(
            || "null".to_string(),
            |value| {
                format!(
                    "{{\"counterexample\":{},\"event\":{}}}",
                    value
                        .counterexample_id
                        .as_deref()
                        .map(|id| format!("\"{}\"", json_escape(id)))
                        .unwrap_or_else(|| "null".to_string()),
                    value
                        .event_id
                        .as_deref()
                        .map(|id| format!("\"{}\"", json_escape(id)))
                        .unwrap_or_else(|| "null".to_string())
                )
            },
        );
        let payload = self
            .payload
            .as_ref()
            .map(DerivationPayload::to_json)
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"assumptions\":[{}],\"claim\":\"{}\",\"disposition\":\"{}\",\"id\":\"{}\",\"identity\":{{\"build\":\"{}\",\"run\":\"{}\",\"source\":\"{}\",\"target\":\"{}\"}},\"method\":\"{}\",\"observation\":{},\"payload\":{},\"premises\":[{}],\"producer\":\"{}\",\"rule\":\"{}\",\"subject\":\"{}\"}}",
            assumptions,
            json_escape(&self.claim),
            self.disposition.as_str(),
            json_escape(&self.id),
            json_escape(&self.identity.build),
            json_escape(&self.identity.run),
            json_escape(&self.identity.source),
            json_escape(&self.identity.target),
            self.method.as_str(),
            observation,
            payload,
            premises,
            json_escape(&self.producer),
            json_escape(&self.rule),
            json_escape(&self.subject),
        )
    }
}


/// Interned link carried by an evidence record.  The payload lives once on
/// `EvidenceReport::derivations`; views should resolve this id rather than copy
/// a proof tree.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DerivationRef {
    pub id: String,
}

impl DerivationRef {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

fn derivation_token(parts: &[&str]) -> String {
    let mut first = 0xcbf29ce484222325u64;
    let mut second = 0x9e3779b185ebca87u64;
    for part in parts {
        let length = (part.len() as u64).to_be_bytes();
        for byte in length.iter().chain(part.as_bytes()) {
            first ^= u64::from(*byte);
            first = first.wrapping_mul(0x100000001b3);
            second ^= first.rotate_left(17) ^ u64::from(*byte);
            second = second.wrapping_mul(0x9e3779b185ebca87);
        }
    }
    format!("{first:016x}{second:016x}")
}

fn json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
