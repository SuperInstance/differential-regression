//! # Differential Regression
//!
//! Regression testing for specification patches using behavioral ledgers.

use simulation_ledger::{Ledger, SimTransaction, replay, DiffEntry};

#[cfg(test)]
use simulation_ledger::diff_ledgers;

// ── failure ─────────────────────────────────────────────────────────────────

/// A single regression failure.
#[derive(Debug, Clone)]
pub struct RegressionFailure {
    pub transaction_id: u64,
    pub label: String,
    pub expected: String,
    pub actual: String,
}

// ── report ──────────────────────────────────────────────────────────────────

/// Summary of a regression run.
#[derive(Debug, Clone)]
pub struct RegressionReport {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub failures: Vec<RegressionFailure>,
}

impl RegressionReport {
    pub fn is_pass(&self) -> bool {
        self.failed == 0
    }
}

// ── runner ──────────────────────────────────────────────────────────────────

/// Replay historical ledgers against a staged specification.
pub fn run_regression<F>(
    ledgers: &[Ledger],
    spec: F,
) -> RegressionReport
where
    F: Fn(&SimTransaction) -> Result<(), String>,
{
    let mut total = 0;
    let mut passed = 0;
    let mut failed = 0;
    let mut failures = Vec::new();

    for ledger in ledgers {
        let result = replay(ledger, &spec);
        total += result.total;
        passed += result.passed;
        failed += result.failed;
        for (id, msg) in &result.failures {
            let label = ledger.get(*id).map(|tx| tx.label.clone()).unwrap_or_default();
            failures.push(RegressionFailure {
                transaction_id: *id,
                label,
                expected: "pass".into(),
                actual: msg.clone(),
            });
        }
    }

    RegressionReport {
        total,
        passed,
        failed,
        failures,
    }
}

// ── gatekeeper ──────────────────────────────────────────────────────────────

/// Decision by the gatekeeper on whether to accept a patch.
#[derive(Debug, Clone, PartialEq)]
pub enum GateDecision {
    Accepted,
    Rejected(String),
}

/// The gatekeeper checks regression reports and ledger diffs to accept or reject a patch.
pub struct Gatekeeper;

impl Gatekeeper {
    /// Accept the patch if the regression report passes and there are no concerning diffs.
    pub fn decide(report: &RegressionReport, diffs: &[DiffEntry]) -> GateDecision {
        if !report.is_pass() {
            let reasons: Vec<String> = report
                .failures
                .iter()
                .map(|f| format!("tx {} ({}): {}", f.transaction_id, f.label, f.actual))
                .collect();
            return GateDecision::Rejected(format!(
                "{} regression failure(s): {}",
                reasons.len(),
                reasons.join("; ")
            ));
        }

        // Check for LeftOnly entries — missing transactions in the new ledger
        let missing: Vec<String> = diffs
            .iter()
            .filter_map(|d| match d {
                DiffEntry::LeftOnly(id, label) => Some(format!("{} ({})", id, label)),
                _ => None,
            })
            .collect();

        if !missing.is_empty() {
            return GateDecision::Rejected(format!(
                "missing transactions in new ledger: {}",
                missing.join(", ")
            ));
        }

        GateDecision::Accepted
    }
}

// ── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ledger(labels: &[&str]) -> Ledger {
        let mut ledger = Ledger::new();
        for label in labels {
            let tx = SimTransaction::new(ledger.len() as u64, label);
            ledger.append_tx(tx);
        }
        ledger
    }

    fn make_ledger_with_outputs(labels: &[(&str, Vec<u8>)]) -> Ledger {
        let mut ledger = Ledger::new();
        for (label, output) in labels {
            let tx = SimTransaction::new(ledger.len() as u64, label).with_output(output.clone());
            ledger.append_tx(tx);
        }
        ledger
    }

    #[test]
    fn test_empty_history_auto_passes() {
        let report = run_regression(&[], |_tx| Ok(()));
        assert!(report.is_pass());
        assert_eq!(report.total, 0);
    }

    #[test]
    fn test_single_step_passes() {
        let ledger = make_ledger(&["step1"]);
        let report = run_regression(&[ledger], |_tx| Ok(()));
        assert!(report.is_pass());
        assert_eq!(report.passed, 1);
    }

    #[test]
    fn test_single_step_fails() {
        let ledger = make_ledger(&["step1"]);
        let report = run_regression(&[ledger], |_tx| Err("bad".into()));
        assert!(!report.is_pass());
        assert_eq!(report.failed, 1);
    }

    #[test]
    fn test_multi_ledger_validation() {
        let l1 = make_ledger(&["a", "b"]);
        let l2 = make_ledger(&["c"]);
        let report = run_regression(&[l1, l2], |tx| {
            if tx.label == "b" {
                Err("b is bad".into())
            } else {
                Ok(())
            }
        });
        assert_eq!(report.total, 3);
        assert_eq!(report.passed, 2);
        assert_eq!(report.failed, 1);
    }

    #[test]
    fn test_gatekeeper_accepts() {
        let report = RegressionReport {
            total: 2,
            passed: 2,
            failed: 0,
            failures: vec![],
        };
        let diffs = vec![DiffEntry::Match(0), DiffEntry::Match(1)];
        assert_eq!(Gatekeeper::decide(&report, &diffs), GateDecision::Accepted);
    }

    #[test]
    fn test_gatekeeper_rejects_on_failure() {
        let report = RegressionReport {
            total: 1,
            passed: 0,
            failed: 1,
            failures: vec![RegressionFailure {
                transaction_id: 0,
                label: "bad".into(),
                expected: "pass".into(),
                actual: "error".into(),
            }],
        };
        let diffs = vec![];
        let decision = Gatekeeper::decide(&report, &diffs);
        assert!(matches!(decision, GateDecision::Rejected(_)));
    }

    #[test]
    fn test_gatekeeper_rejects_missing_tx() {
        let mut left = Ledger::new();
        left.append_tx(SimTransaction::new(0, "a"));
        left.append_tx(SimTransaction::new(1, "b"));
        let mut right = Ledger::new();
        right.append_tx(SimTransaction::new(0, "a"));
        let diffs = diff_ledgers(&left, &right);
        let report = RegressionReport {
            total: 1,
            passed: 1,
            failed: 0,
            failures: vec![],
        };
        let decision = Gatekeeper::decide(&report, &diffs);
        assert!(matches!(decision, GateDecision::Rejected(_)));
    }

    #[test]
    fn test_regression_with_outputs() {
        let ledger = make_ledger_with_outputs(&[
            ("step1", vec![1]),
            ("step2", vec![2]),
            ("step3", vec![0]),
        ]);
        let report = run_regression(&[ledger], |tx| {
            if tx.output.first() == Some(&0) {
                Err("zero not allowed".into())
            } else {
                Ok(())
            }
        });
        assert_eq!(report.failed, 1);
        assert_eq!(report.failures[0].transaction_id, 2);
    }

    #[test]
    fn test_report_is_pass() {
        let report = RegressionReport {
            total: 5,
            passed: 5,
            failed: 0,
            failures: vec![],
        };
        assert!(report.is_pass());
    }

    #[test]
    fn test_gatekeeper_empty_diffs() {
        let report = RegressionReport {
            total: 0,
            passed: 0,
            failed: 0,
            failures: vec![],
        };
        let diffs = vec![];
        assert_eq!(Gatekeeper::decide(&report, &diffs), GateDecision::Accepted);
    }
}
