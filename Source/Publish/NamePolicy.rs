//! Mechanical registry package-name policy (card #1912).

use crate::Diagnostics::Diagnostic;

/// Owner-call thresholds for the warn-then-block edit-distance policy.
///
/// These defaults are deliberately centralized: changing the owner decision
/// changes one policy value and its tests, not the publish path.
pub const NAME_POLICY_WARN_DISTANCE: usize = 2;
pub const NAME_POLICY_BLOCK_DISTANCE: usize = 1;

/// Names ending in one of these suffixes are reserved for registry tooling and
/// must not be published as package names.
pub const RESERVED_SUFFIXES: &[&str] = &["-fixed", "-patched", "-bin"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamePolicyThresholds {
    pub warn_distance: usize,
    pub block_distance: usize,
}

impl Default for NamePolicyThresholds {
    fn default() -> Self {
        Self {
            warn_distance: NAME_POLICY_WARN_DISTANCE,
            block_distance: NAME_POLICY_BLOCK_DISTANCE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamePolicyFinding {
    ReservedSuffix(&'static str),
    Confusable { existing: String },
    EditDistance { existing: String, distance: usize },
}

impl NamePolicyFinding {
    fn reason(&self) -> String {
        match self {
            Self::ReservedSuffix(suffix) => {
                format!("it ends with reserved suffix `{suffix}`")
            }
            Self::Confusable { existing } => {
                format!("it is visually confusable with existing package `{existing}`")
            }
            Self::EditDistance { existing, distance } => {
                format!("it is edit distance {distance} from existing package `{existing}`")
            }
        }
    }

    fn reference(&self) -> String {
        match self {
            Self::ReservedSuffix(suffix) => (*suffix).to_string(),
            Self::Confusable { existing } | Self::EditDistance { existing, .. } => existing.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamePolicyDecision {
    Allow,
    Warn(NamePolicyFinding),
    Block(NamePolicyFinding),
}

impl NamePolicyDecision {
    pub fn is_blocked(&self) -> bool {
        matches!(self, Self::Block(_))
    }

    pub fn is_warning(&self) -> bool {
        matches!(self, Self::Warn(_))
    }

    pub fn finding(&self) -> Option<&NamePolicyFinding> {
        match self {
            Self::Allow => None,
            Self::Warn(finding) | Self::Block(finding) => Some(finding),
        }
    }

    pub fn diagnostic(&self, name: &str) -> Option<Diagnostic> {
        let (code, finding) = match self {
            Self::Allow => return None,
            Self::Warn(finding) => ("L2608", finding),
            Self::Block(finding) => ("E2608", finding),
        };
        let reason = finding.reason();
        let reference = finding.reference();
        Some(Diagnostic::from_row(
            code,
            &[
                ("name", name),
                ("reason", &reason),
                ("reference", &reference),
            ],
            None,
        ))
    }
}

/// Check one candidate against every package name already in the registry.
pub fn assess_name(name: &str, existing_names: &[String]) -> NamePolicyDecision {
    assess_name_with_thresholds(name, existing_names, NamePolicyThresholds::default())
}

/// Check one candidate with an explicit owner-selected threshold pair.
pub fn assess_name_with_thresholds(
    name: &str,
    existing_names: &[String],
    thresholds: NamePolicyThresholds,
) -> NamePolicyDecision {
    let warn_distance = thresholds.warn_distance;
    let block_distance = thresholds.block_distance.min(warn_distance);
    let skeleton = confusable_skeleton(name);

    if let Some(suffix) = RESERVED_SUFFIXES
        .iter()
        .copied()
        .find(|suffix| skeleton.ends_with(suffix))
    {
        return NamePolicyDecision::Block(NamePolicyFinding::ReservedSuffix(suffix));
    }

    let mut nearest_block = None;
    let mut nearest_warning = None;
    for existing in existing_names {
        if existing == name {
            continue;
        }
        let existing_skeleton = confusable_skeleton(existing);
        if existing_skeleton == skeleton {
            return NamePolicyDecision::Block(NamePolicyFinding::Confusable {
                existing: existing.clone(),
            });
        }

        let Some(distance) = edit_distance_at_most(&skeleton, &existing_skeleton, warn_distance)
        else {
            continue;
        };
        let finding = NamePolicyFinding::EditDistance {
            existing: existing.clone(),
            distance,
        };
        if distance <= block_distance {
            if nearest_block
                .as_ref()
                .is_none_or(|(best, current): &(usize, NamePolicyFinding)| {
                    distance < *best
                        || (distance == *best && finding.reference() < current.reference())
                })
            {
                nearest_block = Some((distance, finding));
            }
        } else if nearest_warning.as_ref().is_none_or(
            |(best, current): &(usize, NamePolicyFinding)| {
                distance < *best || (distance == *best && finding.reference() < current.reference())
            },
        ) {
            nearest_warning = Some((distance, finding));
        }
    }

    if let Some((_, finding)) = nearest_block {
        NamePolicyDecision::Block(finding)
    } else if let Some((_, finding)) = nearest_warning {
        NamePolicyDecision::Warn(finding)
    } else {
        NamePolicyDecision::Allow
    }
}

/// Levenshtein distance over Unicode scalar values.
pub fn edit_distance(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    for (row, left_char) in left.iter().enumerate() {
        let mut current = vec![row + 1; right.len() + 1];
        for (column, right_char) in right.iter().enumerate() {
            current[column + 1] = if left_char == right_char {
                previous[column]
            } else {
                1 + previous[column]
                    .min(previous[column + 1])
                    .min(current[column])
            };
        }
        previous = current;
    }
    previous[right.len()]
}

/// Confusable skeleton used by both the exact lookalike and distance checks.
pub fn confusable_skeleton(name: &str) -> String {
    let mut skeleton = String::new();
    for character in name.chars().flat_map(char::to_lowercase) {
        match confusable_char(character) {
            Some(mapped) => skeleton.push_str(mapped),
            None => skeleton.push(character),
        }
    }
    skeleton
}

fn edit_distance_at_most(left: &str, right: &str, limit: usize) -> Option<usize> {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    if left.len().abs_diff(right.len()) > limit {
        return None;
    }
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    for (row, left_char) in left.iter().enumerate() {
        let mut current = vec![limit.saturating_add(1); right.len() + 1];
        current[0] = row + 1;
        let start = row.saturating_sub(limit);
        let end = (row + limit + 1).min(right.len());
        for column in start..end {
            current[column + 1] = if left_char == &right[column] {
                previous[column]
            } else {
                1 + previous[column]
                    .min(previous[column + 1])
                    .min(current[column])
            };
        }
        if current.iter().min().copied().unwrap_or(limit + 1) > limit {
            return None;
        }
        previous = current;
    }
    (previous[right.len()] <= limit).then_some(previous[right.len()])
}

fn confusable_char(character: char) -> Option<&'static str> {
    Some(match character {
        '\u{0300}'..='\u{036f}' => "",
        '0' | 'ο' | 'о' | 'ö' | 'ø' | 'ò' | 'ó' | 'ô' | 'õ' | 'ō' => "o",
        '1' | 'і' | 'ı' | 'ι' => "i",
        'a' | 'а' | 'α' | 'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'b' | 'в' | 'β' => "b",
        'c' | 'с' | 'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
        'd' | 'ԁ' | 'ď' | 'đ' => "d",
        'e' | 'е' | 'ε' | 'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'f' | 'ғ' => "f",
        'g' | 'ɡ' | 'ğ' | 'ĝ' | 'ġ' | 'ģ' => "g",
        'h' | 'н' | 'һ' | 'ĥ' => "h",
        'i' | 'ì' | 'í' | 'î' | 'ï' | 'ī' | 'ĭ' | 'į' | 'ǐ' => "i",
        'j' | 'ј' | 'ĵ' => "j",
        'k' | 'к' | 'ĸ' | 'ķ' => "k",
        'l' | 'ӏ' | 'ł' | 'ĺ' | 'ļ' | 'ľ' | 'ŀ' => "l",
        'm' | 'м' | 'ṁ' | 'ṃ' => "m",
        'n' | 'ո' | 'ñ' | 'ń' | 'ņ' | 'ň' | 'ŋ' => "n",
        'p' | 'р' | 'ρ' | 'ƿ' | 'ṕ' => "p",
        'q' | 'զ' => "q",
        'r' | 'г' | 'ŕ' | 'ŗ' | 'ř' => "r",
        's' | 'ѕ' | 'š' | 'ś' | 'ŝ' | 'ş' | 'ș' => "s",
        't' | 'т' | 'τ' | 'ţ' | 'ť' | 'ŧ' => "t",
        'u' | 'υ' | 'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'v' | 'ν' | 'ѵ' => "v",
        'w' | 'ŵ' => "w",
        'x' | 'х' | 'χ' => "x",
        'y' | 'у' | 'ү' | 'ý' | 'ÿ' | 'ŷ' => "y",
        'z' | 'з' | 'ž' | 'ź' | 'ż' => "z",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn skeleton_collapses_common_homoglyphs() {
        assert_eq!(confusable_skeleton("раypal"), "paypal");
        assert_eq!(confusable_skeleton("librewоlf"), "librewolf");
    }

    #[test]
    fn edit_distance_is_levenshtein() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("cargo", "cagro"), 2);
    }

    #[test]
    fn exact_confusable_match_blocks() {
        let decision = assess_name("librewоlf", &names(&["librewolf"]));
        assert!(matches!(
            decision,
            NamePolicyDecision::Block(NamePolicyFinding::Confusable { .. })
        ));
    }

    #[test]
    fn distance_two_warns_and_distance_one_blocks() {
        let existing = names(&["cargo"]);
        assert!(matches!(
            assess_name("cagro", &existing),
            NamePolicyDecision::Warn(NamePolicyFinding::EditDistance { distance: 2, .. })
        ));
        assert!(matches!(
            assess_name("cargx", &existing),
            NamePolicyDecision::Block(NamePolicyFinding::EditDistance { distance: 1, .. })
        ));
    }

    #[test]
    fn reserved_suffix_blocks_without_existing_name() {
        for suffix in RESERVED_SUFFIXES {
            let name = format!("trusted{suffix}");
            assert!(matches!(
                assess_name(&name, &[]),
                NamePolicyDecision::Block(NamePolicyFinding::ReservedSuffix(_))
            ));
        }
    }

    #[test]
    fn unrelated_name_is_allowed() {
        assert_eq!(
            assess_name("textkit", &names(&["cargo", "jetpack"])),
            NamePolicyDecision::Allow
        );
    }

    #[test]
    fn warning_diagnostic_matches_ui_snapshot() {
        let decision = assess_name("cagro", &names(&["cargo"]));
        let diagnostic = decision.diagnostic("cagro").expect("warning diagnostic");
        assert_eq!(
            diagnostic.render_colored("", "", false),
            include_str!("../../tests/cli/registry_name_policy_warning.txt")
        );
    }
}
