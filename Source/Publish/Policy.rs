//! Package metadata policy (D-JPK-POLICYSURFACE1=D).
//!
//! This is the one resolver-side gate for SPDX metadata and package-pattern
//! source mapping. It returns a receipt instead of a boolean so the exact
//! rule, evidence, and policy fingerprint can travel into Hangar provenance
//! and semantic-lock rationale.

use crate::Diagnostics::Diagnostic;
use crate::Package::{PackagePolicy, PackagePolicyException};
use crate::SHA256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagePolicyReceipt {
    pub package: String,
    pub version: String,
    pub license: String,
    pub source: String,
    pub source_rule: String,
    pub exception: Option<String>,
    pub fingerprint: String,
}

impl PackagePolicyReceipt {
    pub fn summary(&self) -> String {
        let source_rule = if self.source_rule.is_empty() {
            "default"
        } else {
            self.source_rule.as_str()
        };
        let exception = self
            .exception
            .as_deref()
            .map(|value| format!(";exception={value}"))
            .unwrap_or_default();
        format!(
            "package={}#{};license={};source={};source-rule={};fingerprint={}{}",
            self.package,
            self.version,
            self.license,
            self.source,
            source_rule,
            self.fingerprint,
            exception
        )
    }
}

/// Return the one source exception that is active for this exact package
/// identity. Expiry is part of the authorization decision, not just display
/// metadata; stale exceptions must not enter receipts, locks, or provenance.
pub(crate) fn active_source_exception<'a>(
    exceptions: &'a [PackagePolicyException],
    package: &str,
    version: &str,
) -> Option<&'a PackagePolicyException> {
    let now = super::advisory_now();
    exceptions
        .iter()
        .find(|exception| exception.matches(package, version) && exception.expires_at > now)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagePolicyError {
    pub detail: String,
    pub fix: String,
}

impl PackagePolicyError {
    fn new(detail: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
            fix: fix.into(),
        }
    }
}

/// Validate and authorize one already hash-verified package candidate.
/// Metadata is checked before the candidate is ingested into Hangar.
pub fn authorize_package_candidate(
    policy: &PackagePolicy,
    package: &str,
    version: &str,
    license: Option<&str>,
    source: &str,
) -> Result<PackagePolicyReceipt, PackagePolicyError> {
    let license = license.map(str::trim).filter(|value| !value.is_empty()).ok_or_else(|| {
        PackagePolicyError::new(
            format!("`{package}#{version}` has no SPDX license expression"),
            "add a valid SPDX expression to the dependency's `license:` field, or use a reviewed source with complete metadata",
        )
    })?;
    let identifiers = parse_spdx_expression(license).map_err(|detail| {
        PackagePolicyError::new(
            format!("`{package}#{version}` has invalid SPDX metadata: {detail}"),
            "publish a valid SPDX expression such as `MIT` or `MIT OR Apache-2.0`, then refresh the trusted registry metadata",
        )
    })?;
    if identifiers
        .iter()
        .any(|id| matches!(id.as_str(), "NONE" | "NOASSERTION"))
    {
        return Err(PackagePolicyError::new(
            format!("`{package}#{version}` does not make a usable SPDX license claim"),
            "publish a concrete SPDX license expression; `NONE` and `NOASSERTION` cannot satisfy package policy",
        ));
    }
    if let Some(allowed) = &policy.licenses {
        let allowed = allowed
            .iter()
            .map(|value| {
                parse_spdx_expression(value)
                    .map(|ids| ids.into_iter().collect::<Vec<_>>())
                    .map_err(|detail| (value.clone(), detail))
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|(value, detail)| {
                PackagePolicyError::new(
                    format!("package policy has invalid SPDX allow-list entry `{value}`: {detail}"),
                    "use SPDX identifiers in `policy.licenses: .Allow([...])`",
                )
            })?;
        for identifier in &identifiers {
            if !allowed
                .iter()
                .any(|entry| entry.iter().any(|value| value == identifier))
            {
                return Err(PackagePolicyError::new(
                    format!(
                        "`{package}#{version}` license `{identifier}` is outside the SPDX allow-list"
                    ),
                    format!(
                        "add `{identifier}` to `policy.licenses: .Allow([...])`, or use a package whose license is already allowed"
                    ),
                ));
            }
        }
    }

    let source_rule = if policy.source_maps.is_empty() {
        String::new()
    } else {
        let (_, sources) = matching_source_rule(&policy.source_maps, package).ok_or_else(|| {
            PackagePolicyError::new(
                format!("`{package}#{version}` has no source mapping for authority `{source}`"),
                format!("add a `policy.sources` rule for `{package}` that names `{source}`"),
            )
        })?;
        if !sources.iter().any(|allowed| allowed == source) {
            return Err(PackagePolicyError::new(
                format!(
                    "`{package}#{version}` source authority `{source}` is not allowed by its source mapping"
                ),
                format!(
                    "use an allowed source for `{package}`, or add `{source}` to its `policy.sources` rule"
                ),
            ));
        }
        let (pattern, _) = matching_source_rule(&policy.source_maps, package)
            .expect("source mapping was matched above");
        format!("{pattern} => [{}]", sources.join(", "))
    };

    Ok(PackagePolicyReceipt {
        package: package.to_string(),
        version: version.to_string(),
        license: license.to_string(),
        source: source.to_string(),
        source_rule,
        exception: active_source_exception(&policy.exceptions, package, version)
            .map(PackagePolicyException::summary),
        fingerprint: policy_fingerprint(policy),
    })
}

/// Validate one package's SPDX expression under the safe default policy.
pub fn validate_spdx_license(license: Option<&str>) -> Result<(), PackagePolicyError> {
    validate_published_license("published-package", "unknown", license).map(|_| ())
}

pub fn validate_published_license(
    package: &str,
    version: &str,
    license: Option<&str>,
) -> Result<PackagePolicyReceipt, PackagePolicyError> {
    authorize_package_candidate(
        &PackagePolicy::default(),
        package,
        version,
        license,
        "local",
    )
}

pub fn package_policy_diagnostic(error: &PackagePolicyError) -> Diagnostic {
    Diagnostic::error(
        "E2607",
        format!("source-owned package policy rejected the candidate: {}", error.detail),
        "the package source owns the requested license and source policy; this security-sensitive evidence is rejected before ingest".to_string(),
        error.fix.clone(),
        None,
    )
}

/// Explain a package-policy denial at the dependency edge that supplied it.
/// The resolver owns the graph context; the policy gate owns the evidence and
/// smallest source fix. Keeping both here prevents a generic registry error
/// from hiding which source declaration must change.
pub(crate) fn package_policy_edge_diagnostic(
    owner: &str,
    edge: &str,
    source: &str,
    error: &PackagePolicyError,
) -> Diagnostic {
    Diagnostic::error(
        "E1207",
        format!(
            "source-owned package policy rejected dependency edge `{edge}`: {}",
            error.detail
        ),
        format!(
            "package `{owner}` requested source authority `{source}`; policy evidence is evaluated after registry identity and before Hangar ingest"
        ),
        error.fix.clone(),
        None,
    )
}

pub fn policy_fingerprint(policy: &PackagePolicy) -> String {
    let licenses = policy.licenses.as_ref().map(|values| {
        let mut values = values.clone();
        values.sort();
        values
    });
    let mut source_maps = policy.source_maps.clone();
    for (_, sources) in &mut source_maps {
        sources.sort();
    }
    source_maps.sort_by(|left, right| left.0.cmp(&right.0));
    let mut exceptions = policy.exceptions.clone();
    exceptions.sort();
    let canonical = format!(
        "licenses={:?};sources={:?};exceptions={:?}",
        licenses, source_maps, exceptions
    );
    format!("sha256-{}", SHA256::sha256_hex(canonical.as_bytes()))
}

fn matching_source_rule<'a>(
    rules: &'a [(String, Vec<String>)],
    package: &str,
) -> Option<(&'a str, &'a Vec<String>)> {
    rules
        .iter()
        .filter_map(|(pattern, sources)| {
            pattern_matches(pattern, package).then_some((pattern.as_str(), sources))
        })
        .max_by_key(|(pattern, _)| {
            (
                pattern.strip_suffix('*').unwrap_or(pattern).len(),
                !pattern.ends_with('*'),
            )
        })
}

fn pattern_matches(pattern: &str, package: &str) -> bool {
    pattern
        .strip_suffix('*')
        .map(|prefix| package.starts_with(prefix))
        .unwrap_or_else(|| pattern == package)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Id(String),
    And,
    Or,
    With,
    Open,
    Close,
}

fn parse_spdx_expression(raw: &str) -> Result<Vec<String>, String> {
    let tokens = tokenize(raw)?;
    if tokens.is_empty() {
        return Err("the expression is empty".to_string());
    }
    let mut parser = Parser { tokens, index: 0 };
    let mut identifiers = parser.expression()?;
    if parser.index != parser.tokens.len() {
        return Err("expected an SPDX operator or closing parenthesis".to_string());
    }
    identifiers.sort();
    identifiers.dedup();
    Ok(identifiers)
}

fn tokenize(raw: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = raw.chars().peekable();
    while let Some(character) = chars.peek().copied() {
        if character.is_whitespace() {
            chars.next();
            continue;
        }
        match character {
            '(' => {
                chars.next();
                tokens.push(Token::Open);
            }
            ')' => {
                chars.next();
                tokens.push(Token::Close);
            }
            _ => {
                let mut value = String::new();
                while let Some(character) = chars.peek().copied() {
                    if character.is_whitespace() || matches!(character, '(' | ')') {
                        break;
                    }
                    if !character.is_ascii_alphanumeric()
                        && !matches!(character, '.' | '-' | '+' | ':')
                    {
                        return Err(format!("unsupported character `{character}`"));
                    }
                    value.push(character);
                    chars.next();
                }
                if value.is_empty() {
                    return Err("empty SPDX token".to_string());
                }
                tokens.push(match value.as_str() {
                    "AND" => Token::And,
                    "OR" => Token::Or,
                    "WITH" => Token::With,
                    _ => Token::Id(value),
                });
            }
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
}

impl Parser {
    fn expression(&mut self) -> Result<Vec<String>, String> {
        self.or_expression()
    }

    fn or_expression(&mut self) -> Result<Vec<String>, String> {
        let mut ids = self.and_expression()?;
        while self.take(Token::Or) {
            ids.extend(self.and_expression()?);
        }
        Ok(ids)
    }

    fn and_expression(&mut self) -> Result<Vec<String>, String> {
        let mut ids = self.with_expression()?;
        while self.take(Token::And) {
            ids.extend(self.with_expression()?);
        }
        Ok(ids)
    }

    fn with_expression(&mut self) -> Result<Vec<String>, String> {
        let mut ids = self.primary()?;
        if self.take(Token::With) {
            let exception = self.next_id()?;
            ids.push(exception);
        }
        Ok(ids)
    }

    fn primary(&mut self) -> Result<Vec<String>, String> {
        match self.tokens.get(self.index).cloned() {
            Some(Token::Id(id)) => {
                self.index += 1;
                Ok(vec![id])
            }
            Some(Token::Open) => {
                self.index += 1;
                let ids = self.expression()?;
                if !self.take(Token::Close) {
                    return Err("missing closing parenthesis".to_string());
                }
                Ok(ids)
            }
            Some(Token::Close) => Err("unexpected closing parenthesis".to_string()),
            Some(Token::And | Token::Or | Token::With) | None => {
                Err("expected an SPDX license identifier".to_string())
            }
        }
    }

    fn next_id(&mut self) -> Result<String, String> {
        match self.tokens.get(self.index).cloned() {
            Some(Token::Id(id)) => {
                self.index += 1;
                Ok(id)
            }
            _ => Err("`WITH` must be followed by an SPDX exception identifier".to_string()),
        }
    }

    fn take(&mut self, expected: Token) -> bool {
        if self.tokens.get(self.index) == Some(&expected) {
            self.index += 1;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{authorize_package_candidate, validate_spdx_license};
    use crate::Package::PackagePolicy;

    #[test]
    fn valid_spdx_and_mapped_source_emit_receipt() {
        let policy = PackagePolicy {
            licenses: Some(vec!["MIT".to_string(), "Apache-2.0".to_string()]),
            source_maps: vec![("Acme.*".to_string(), vec!["internal".to_string()])],
            ..Default::default()
        };
        let receipt = authorize_package_candidate(
            &policy,
            "Acme.Widget",
            "1.2.3",
            Some("MIT OR Apache-2.0"),
            "internal",
        )
        .expect("policy should allow the mapped candidate");
        assert!(receipt.summary().contains("source-rule=Acme.*"));
        assert!(receipt.fingerprint.starts_with("sha256-"));
    }

    #[test]
    fn invalid_license_and_unmapped_source_fail_closed() {
        assert!(validate_spdx_license(Some("not a license")).is_err());
        let policy = PackagePolicy {
            licenses: Some(vec!["MIT".to_string()]),
            source_maps: vec![("widget".to_string(), vec!["internal".to_string()])],
            ..Default::default()
        };
        assert!(
            authorize_package_candidate(&policy, "widget", "1.0.0", Some("MIT"), "public",)
                .is_err()
        );
    }
}
