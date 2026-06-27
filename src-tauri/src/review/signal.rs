use super::{ReviewComment, ReviewFocus, Severity};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum SignalTier {
    #[serde(rename = "tier_1", alias = "Tier1", alias = "tier1", alias = "TIER_1")]
    Tier1,
    #[serde(rename = "tier_2", alias = "Tier2", alias = "tier2", alias = "TIER_2")]
    Tier2,
    #[serde(rename = "noise", alias = "Noise", alias = "NOISE")]
    Noise,
    #[serde(
        rename = "unclassified",
        alias = "Unclassified",
        alias = "UNCLASSIFIED"
    )]
    Unclassified,
}

impl Default for SignalTier {
    fn default() -> Self {
        Self::Unclassified
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignalRules {
    #[serde(default)]
    pub tier1_cwes: Vec<String>,
    #[serde(default)]
    pub tier2_cwes: Vec<String>,
    #[serde(default)]
    pub noise_cwes: Vec<String>,
    #[serde(default)]
    pub tier1_categories: Vec<String>,
    #[serde(default)]
    pub tier2_categories: Vec<String>,
    #[serde(default)]
    pub noise_categories: Vec<String>,
    #[serde(default, alias = "focus_rules")]
    pub per_focus: BTreeMap<ReviewFocus, FocusSignalRules>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct FocusSignalRules {
    #[serde(default)]
    pub tier1_cwes: Vec<String>,
    #[serde(default)]
    pub tier2_cwes: Vec<String>,
    #[serde(default)]
    pub noise_cwes: Vec<String>,
    #[serde(default)]
    pub tier1_categories: Vec<String>,
    #[serde(default)]
    pub tier2_categories: Vec<String>,
    #[serde(default)]
    pub noise_categories: Vec<String>,
}

impl Default for SignalRules {
    fn default() -> Self {
        Self {
            tier1_cwes: vec![
                "CWE-22", "CWE-78", "CWE-79", "CWE-89", "CWE-269", "CWE-287", "CWE-288", "CWE-306",
                "CWE-434", "CWE-502", "CWE-611", "CWE-798", "CWE-918",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            tier2_cwes: vec![
                "CWE-200", "CWE-352", "CWE-522", "CWE-532", "CWE-601", "CWE-770",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            noise_cwes: Vec::new(),
            tier1_categories: vec![
                "access control",
                "authentication",
                "authorization",
                "command injection",
                "credential",
                "deserialization",
                "injection",
                "path traversal",
                "secret",
                "sql injection",
                "ssrf",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            tier2_categories: vec![
                "csrf",
                "cryptography",
                "denial of service",
                "information disclosure",
                "input validation",
                "rate limit",
                "xss",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            noise_categories: vec![
                "best practice",
                "documentation",
                "formatting",
                "lint",
                "maintainability",
                "performance",
                "style",
                "test",
                "tests",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            per_focus: BTreeMap::new(),
        }
    }
}

impl SignalRules {
    pub fn merge(&mut self, override_rules: PartialSignalRules) {
        extend_unique_cwes(&mut self.tier1_cwes, override_rules.tier1_cwes);
        extend_unique_cwes(&mut self.tier2_cwes, override_rules.tier2_cwes);
        extend_unique_cwes(&mut self.noise_cwes, override_rules.noise_cwes);
        extend_unique_categories(&mut self.tier1_categories, override_rules.tier1_categories);
        extend_unique_categories(&mut self.tier2_categories, override_rules.tier2_categories);
        extend_unique_categories(&mut self.noise_categories, override_rules.noise_categories);
        if let Some(per_focus) = override_rules.per_focus {
            for (focus, rules) in per_focus {
                self.per_focus.entry(focus).or_default().merge(rules);
            }
        }
    }
}

impl FocusSignalRules {
    fn merge(&mut self, override_rules: PartialFocusSignalRules) {
        extend_unique_cwes(&mut self.tier1_cwes, override_rules.tier1_cwes);
        extend_unique_cwes(&mut self.tier2_cwes, override_rules.tier2_cwes);
        extend_unique_cwes(&mut self.noise_cwes, override_rules.noise_cwes);
        extend_unique_categories(&mut self.tier1_categories, override_rules.tier1_categories);
        extend_unique_categories(&mut self.tier2_categories, override_rules.tier2_categories);
        extend_unique_categories(&mut self.noise_categories, override_rules.noise_categories);
    }
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub struct PartialSignalRules {
    pub tier1_cwes: Option<Vec<String>>,
    pub tier2_cwes: Option<Vec<String>>,
    pub noise_cwes: Option<Vec<String>>,
    pub tier1_categories: Option<Vec<String>>,
    pub tier2_categories: Option<Vec<String>>,
    pub noise_categories: Option<Vec<String>>,
    #[serde(default, alias = "focus_rules")]
    pub per_focus: Option<BTreeMap<ReviewFocus, PartialFocusSignalRules>>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub struct PartialFocusSignalRules {
    pub tier1_cwes: Option<Vec<String>>,
    pub tier2_cwes: Option<Vec<String>>,
    pub noise_cwes: Option<Vec<String>>,
    pub tier1_categories: Option<Vec<String>>,
    pub tier2_categories: Option<Vec<String>>,
    pub noise_categories: Option<Vec<String>>,
}

pub fn normalize_review_comment(comment: &mut ReviewComment, rules: &SignalRules) {
    comment.signal_tier = classify_comment(comment, rules);
    comment.comment_id = comment_id_for(comment);
}

pub fn classify_comment(comment: &ReviewComment, rules: &SignalRules) -> SignalTier {
    let cwe_id = comment.cwe_id.as_deref().map(normalize_cwe);
    let category = normalize_category(&comment.category);
    let focus_subcategory = comment
        .focus_subcategory
        .as_deref()
        .map(normalize_category)
        .filter(|value| !value.is_empty());
    let focus_rules = rules.per_focus.get(&comment.focus);

    let cwe_in = |base: &[String], focused: fn(&FocusSignalRules) -> &[String]| {
        cwe_matches(cwe_id.as_deref(), base)
            || focus_rules
                .map(|rules| cwe_matches(cwe_id.as_deref(), focused(rules)))
                .unwrap_or(false)
    };
    let category_in = |base: &[String], focused: fn(&FocusSignalRules) -> &[String]| {
        category_matches(&category, None, base)
            || focus_rules
                .map(|rules| {
                    category_matches(&category, focus_subcategory.as_deref(), focused(rules))
                })
                .unwrap_or(false)
    };

    if comment.severity == Severity::Critical {
        return SignalTier::Tier1;
    }

    if cwe_in(&rules.tier1_cwes, |rules| &rules.tier1_cwes)
        || category_in(&rules.tier1_categories, |rules| &rules.tier1_categories)
    {
        if matches!(comment.severity, Severity::High | Severity::Medium) {
            return SignalTier::Tier1;
        }
    }

    if matches!(comment.severity, Severity::Low | Severity::Info)
        && (cwe_in(&rules.noise_cwes, |rules| &rules.noise_cwes)
            || category_in(&rules.noise_categories, |rules| &rules.noise_categories))
    {
        return SignalTier::Noise;
    }

    if comment.signal_tier == SignalTier::Unclassified
        && matches!(comment.severity, Severity::High | Severity::Medium)
        && (cwe_in(&rules.tier2_cwes, |rules| &rules.tier2_cwes)
            || category_in(&rules.tier2_categories, |rules| &rules.tier2_categories))
    {
        return SignalTier::Tier2;
    }

    comment.signal_tier
}

pub fn is_actionable(tier: SignalTier) -> bool {
    matches!(tier, SignalTier::Tier1 | SignalTier::Tier2)
}

pub fn snr_percent(comments: &[ReviewComment]) -> f64 {
    if comments.is_empty() {
        return 100.0;
    }

    let actionable = comments
        .iter()
        .filter(|comment| is_actionable(comment.signal_tier))
        .count();
    percentage(actionable, comments.len())
}

pub fn percentage(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        return 100.0;
    }

    ((numerator as f64 / denominator as f64) * 1000.0).round() / 10.0
}

pub fn comment_id_for(comment: &ReviewComment) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"gospel-review-comment-v1");
    update_length_prefixed(&mut hasher, &normalize_file_anchor(&comment.file));
    hasher.update((comment.line_start as u64).to_le_bytes());
    hasher.update((comment.line_end as u64).to_le_bytes());
    update_length_prefixed(&mut hasher, &comment.title);
    update_length_prefixed(&mut hasher, &comment.evidence);
    let digest = hex_digest(hasher.finalize().as_slice());
    format!("rc_{}", &digest[..24])
}

fn extend_unique_cwes(target: &mut Vec<String>, values: Option<Vec<String>>) {
    extend_unique_with(target, values, normalize_cwe);
}

fn extend_unique_categories(target: &mut Vec<String>, values: Option<Vec<String>>) {
    extend_unique_with(target, values, normalize_category);
}

fn extend_unique_with(
    target: &mut Vec<String>,
    values: Option<Vec<String>>,
    normalize: impl Fn(&str) -> String,
) {
    let Some(values) = values else {
        return;
    };

    let mut seen = target
        .iter()
        .map(|value| normalize(value))
        .collect::<BTreeSet<_>>();
    for value in values {
        let key = normalize(&value);
        if seen.insert(key) {
            target.push(value);
        }
    }
}

fn normalize_cwe(value: &str) -> String {
    value.trim().to_ascii_uppercase().replace(' ', "")
}

fn normalize_category(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace(['_', '-'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn cwe_matches(cwe_id: Option<&str>, values: &[String]) -> bool {
    cwe_id
        .map(|id| values.iter().any(|value| normalize_cwe(value) == id))
        .unwrap_or(false)
}

fn category_matches(category: &str, focus_subcategory: Option<&str>, values: &[String]) -> bool {
    values
        .iter()
        .map(|value| normalize_category(value))
        .any(|rule| {
            !rule.is_empty()
                && (category.contains(&rule)
                    || focus_subcategory
                        .map(|subcategory| subcategory.contains(&rule))
                        .unwrap_or(false))
        })
}

fn normalize_file_anchor(file: &str) -> String {
    file.trim_start_matches("./").replace('\\', "/")
}

fn update_length_prefixed(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment(
        severity: Severity,
        cwe_id: Option<&str>,
        category: &str,
        tier: SignalTier,
    ) -> ReviewComment {
        ReviewComment {
            file: "src/main.rs".to_string(),
            line_start: 10,
            line_end: 12,
            severity,
            category: category.to_string(),
            focus: ReviewFocus::Security,
            focus_subcategory: None,
            cwe_id: cwe_id.map(str::to_string),
            cwe_name: None,
            title: "Finding".to_string(),
            description: "Description".to_string(),
            rationale: Some("Rationale".to_string()),
            evidence: "dangerous_call(input)".to_string(),
            suggestion: Some("Fix it".to_string()),
            verification_plan: Some("Run a test".to_string()),
            signal_tier: tier,
            comment_id: String::new(),
        }
    }

    #[test]
    fn guardrail_promotes_critical_command_injection_to_tier1() {
        let mut finding = comment(
            Severity::Critical,
            Some("CWE-78"),
            "style",
            SignalTier::Noise,
        );

        normalize_review_comment(&mut finding, &SignalRules::default());

        assert_eq!(finding.signal_tier, SignalTier::Tier1);
        assert!(finding.comment_id.starts_with("rc_"));
    }

    #[test]
    fn guardrail_demotes_low_style_comments_to_noise() {
        let finding = comment(Severity::Low, None, "style", SignalTier::Tier2);

        assert_eq!(
            classify_comment(&finding, &SignalRules::default()),
            SignalTier::Noise
        );
    }

    #[test]
    fn guardrail_preserves_existing_tier_without_matching_rule() {
        let finding = comment(Severity::Medium, None, "business logic", SignalTier::Tier2);

        assert_eq!(
            classify_comment(&finding, &SignalRules::default()),
            SignalTier::Tier2
        );
    }

    #[test]
    fn tier2_rules_promote_unclassified_high_or_medium_findings() {
        let finding = comment(
            Severity::High,
            Some("CWE-200"),
            "style",
            SignalTier::Unclassified,
        );

        assert_eq!(
            classify_comment(&finding, &SignalRules::default()),
            SignalTier::Tier2
        );
    }

    #[test]
    fn tier2_rules_preserve_explicit_model_tiers() {
        let finding = comment(Severity::High, Some("CWE-200"), "style", SignalTier::Noise);

        assert_eq!(
            classify_comment(&finding, &SignalRules::default()),
            SignalTier::Noise
        );
    }

    #[test]
    fn merge_deduplicates_cwes_with_classifier_normalization() {
        let mut rules = SignalRules {
            tier1_cwes: vec!["CWE 78".to_string()],
            tier2_cwes: Vec::new(),
            noise_cwes: Vec::new(),
            tier1_categories: Vec::new(),
            tier2_categories: Vec::new(),
            noise_categories: Vec::new(),
            per_focus: BTreeMap::new(),
        };

        rules.merge(PartialSignalRules {
            tier1_cwes: Some(vec!["cwe 78".to_string(), "CWE-78".to_string()]),
            ..PartialSignalRules::default()
        });

        assert_eq!(rules.tier1_cwes, vec!["CWE 78", "CWE-78"]);
    }

    #[test]
    fn normalize_overrides_model_supplied_comment_id_with_deterministic_hash() {
        let mut finding = comment(
            Severity::High,
            Some("CWE-78"),
            "injection",
            SignalTier::Tier1,
        );
        let model_id = "rc_unstable_or_hallucinated".to_string();
        finding.comment_id = model_id.clone();

        normalize_review_comment(&mut finding, &SignalRules::default());

        assert_ne!(finding.comment_id, model_id);
        assert_eq!(finding.comment_id, comment_id_for(&finding));
        assert!(finding.comment_id.starts_with("rc_"));
    }

    #[test]
    fn classify_comment_uses_focus_specific_categories() {
        let mut rules = SignalRules::default();
        rules.merge(PartialSignalRules {
            per_focus: Some(BTreeMap::from([(
                ReviewFocus::BugHunt,
                PartialFocusSignalRules {
                    tier1_categories: Some(vec!["state transition".to_string()]),
                    ..PartialFocusSignalRules::default()
                },
            )])),
            ..PartialSignalRules::default()
        });
        let mut finding = comment(
            Severity::Medium,
            None,
            "correctness",
            SignalTier::Unclassified,
        );
        finding.focus = ReviewFocus::BugHunt;
        finding.focus_subcategory = Some("state transition".to_string());

        assert_eq!(classify_comment(&finding, &rules), SignalTier::Tier1);

        finding.focus = ReviewFocus::Security;
        assert_eq!(classify_comment(&finding, &rules), SignalTier::Unclassified);
    }

    #[test]
    fn focus_subcategory_does_not_match_global_categories() {
        let mut finding = comment(
            Severity::Medium,
            None,
            "correctness",
            SignalTier::Unclassified,
        );
        finding.focus = ReviewFocus::BugHunt;
        finding.focus_subcategory = Some("authorization".to_string());

        assert_eq!(
            classify_comment(&finding, &SignalRules::default()),
            SignalTier::Unclassified
        );
    }

    #[test]
    fn classify_comment_legacy_security_unchanged() {
        let tier1 = comment(
            Severity::High,
            Some("CWE-78"),
            "command injection",
            SignalTier::Unclassified,
        );
        let noise = comment(Severity::Low, None, "style", SignalTier::Tier2);

        assert_eq!(
            classify_comment(&tier1, &SignalRules::default()),
            SignalTier::Tier1
        );
        assert_eq!(
            classify_comment(&noise, &SignalRules::default()),
            SignalTier::Noise
        );
    }
}
