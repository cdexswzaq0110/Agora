//! Evidence grades — the same five-grade vocabulary as Serendipity-Epiphany's
//! `evidence-grades.md` and Agent Kernel's `EvidenceGrade`. Ordered weakest → strongest so
//! `min()` gives "the weakest link".

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceGrade {
    Unknown,
    Unverified,
    Candidate,
    Inferred,
    Confirmed,
}

impl EvidenceGrade {
    pub fn label(self) -> &'static str {
        match self {
            EvidenceGrade::Confirmed => "已確認",
            EvidenceGrade::Inferred => "推論",
            EvidenceGrade::Candidate => "候選",
            EvidenceGrade::Unknown => "未知",
            EvidenceGrade::Unverified => "未驗證",
        }
    }

    /// Parse either the Chinese label or the English name.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().trim_matches(|c: char| "【】[]`*：: <>".contains(c));
        let lower = s.to_ascii_lowercase();
        if s.starts_with("已確認") || lower.starts_with("confirmed") {
            Some(EvidenceGrade::Confirmed)
        } else if s.starts_with("推論") || lower.starts_with("inferred") {
            Some(EvidenceGrade::Inferred)
        } else if s.starts_with("候選") || lower.starts_with("candidate") {
            Some(EvidenceGrade::Candidate)
        } else if s.starts_with("未知") || lower.starts_with("unknown") {
            Some(EvidenceGrade::Unknown)
        } else if s.starts_with("未驗證") || lower.starts_with("unverified") {
            Some(EvidenceGrade::Unverified)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_labels_and_orders_weakest_first() {
        assert_eq!(EvidenceGrade::parse("【推論】"), Some(EvidenceGrade::Inferred));
        assert_eq!(EvidenceGrade::parse(" Confirmed "), Some(EvidenceGrade::Confirmed));
        assert_eq!(EvidenceGrade::parse("未驗證。"), Some(EvidenceGrade::Unverified));
        assert_eq!(EvidenceGrade::parse("maybe"), None);
        assert!(EvidenceGrade::Confirmed > EvidenceGrade::Inferred);
        assert_eq!(
            EvidenceGrade::Confirmed.min(EvidenceGrade::Candidate),
            EvidenceGrade::Candidate
        );
    }
}
