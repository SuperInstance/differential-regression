//! # differential-regression — The Gatekeeper
//!
//! Before any refined specification patch is allowed to merge, the engine treats
//! the historical ledger of SimTransactions as an immutable suite of black-box
//! integration tests. The runner executes a two-phase check:
//!
//! 1. **Simulation Replay**: Replays every historical input event. If the new
//!    spec generates output that conflicts with an approved baseline, it flags
//!    a Semantic Regression.
//!
//! 2. **Dual-Execution Validation**: Compiles the patch and dual-executes both
//!    the simulation and the compiled binary side-by-side against the ledger.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

// ─── SimTransaction ──────────────────────────────────────────────────────────

/// A single simulation transaction — the irreducible unit of behavioral capture.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SimTransaction {
    pub input_event: String,
    pub simulated_output: String,
    #[serde(default)]
    pub state_mutations: HashMap<String, String>,
}

impl SimTransaction {
    pub fn new(input: &str, output: &str) -> Self {
        Self {
            input_event: input.to_string(),
            simulated_output: output.to_string(),
            state_mutations: HashMap::new(),
        }
    }

    pub fn with_mutations(input: &str, output: &str, m: HashMap<String, String>) -> Self {
        Self { input_event: input.to_string(), simulated_output: output.to_string(), state_mutations: m }
    }
}

// ─── Failure Tracking ────────────────────────────────────────────────────────

/// A single regression failure — where proposed output diverged from historical baseline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegressionFailure {
    pub ledger_file: String,
    pub step_index: usize,
    pub input_trigger: String,
    pub expected_output: String,
    pub actual_output: String,
}

impl fmt::Display for RegressionFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[FAIL] {} step {}: expected '{}' got '{}'",
            self.ledger_file, self.step_index, self.expected_output, self.actual_output)
    }
}

// ─── Report ──────────────────────────────────────────────────────────────────

/// Summary of a regression run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegressionReport {
    pub total_ledgers: usize,
    pub total_assertions: usize,
    pub passed: usize,
    pub failed: usize,
    pub failures: Vec<RegressionFailure>,
}

impl RegressionReport {
    pub fn is_clean(&self) -> bool {
        self.failures.is_empty()
    }

    pub fn pass_rate(&self) -> f64 {
        if self.total_assertions == 0 { return 1.0; }
        self.passed as f64 / self.total_assertions as f64
    }
}

impl fmt::Display for RegressionReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Regression: {} ledgers, {} assertions, {} passed, {} failed ({:.1}%)",
            self.total_ledgers, self.total_assertions, self.passed, self.failed, self.pass_rate() * 100.0)
    }
}

// ─── Regression Runner ───────────────────────────────────────────────────────

/// The core regression engine: loads historical ledgers and validates proposed specs.
pub struct DifferentialRegressionRunner {
    /// Historical ledgers: name → list of transactions
    ledgers: HashMap<String, Vec<SimTransaction>>,
}

impl DifferentialRegressionRunner {
    /// Create a new runner with no history.
    pub fn new() -> Self {
        Self { ledgers: HashMap::new() }
    }

    /// Create with pre-loaded history.
    pub fn with_history(ledgers: HashMap<String, Vec<SimTransaction>>) -> Self {
        Self { ledgers }
    }

    /// Add a historical ledger.
    pub fn add_ledger(&mut self, name: &str, transactions: Vec<SimTransaction>) {
        self.ledgers.insert(name.to_string(), transactions);
    }

    /// Run regression check against a proposed spec.
    /// The `simulate_fn` closure takes an input event and returns the proposed output.
    pub fn run<F>(&self, simulate_fn: F) -> RegressionReport
    where
        F: Fn(&str) -> String,
    {
        let mut report = RegressionReport {
            total_ledgers: self.ledgers.len(),
            total_assertions: 0,
            passed: 0,
            failed: 0,
            failures: Vec::new(),
        };

        for (ledger_name, transactions) in &self.ledgers {
            for (idx, tx) in transactions.iter().enumerate() {
                report.total_assertions += 1;
                let actual = simulate_fn(&tx.input_event);

                if actual.trim() == tx.simulated_output.trim() {
                    report.passed += 1;
                } else {
                    report.failed += 1;
                    report.failures.push(RegressionFailure {
                        ledger_file: ledger_name.clone(),
                        step_index: idx,
                        input_trigger: tx.input_event.clone(),
                        expected_output: tx.simulated_output.clone(),
                        actual_output: actual,
                    });
                }
            }
        }

        report
    }

    /// Run regression with a simple mapping of inputs to outputs.
    pub fn run_against_map(&self, proposed: &HashMap<String, String>) -> RegressionReport {
        self.run(|input| {
            proposed.get(input).cloned().unwrap_or_default()
        })
    }

    /// Number of ledgers loaded.
    pub fn ledger_count(&self) -> usize {
        self.ledgers.len()
    }

    /// Total transactions across all ledgers.
    pub fn total_transactions(&self) -> usize {
        self.ledgers.values().map(|v| v.len()).sum()
    }
}

impl Default for DifferentialRegressionRunner {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Gatekeeper ──────────────────────────────────────────────────────────────

/// The gatekeeper accepts or rejects a patch based on regression results.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GatekeeperDecision {
    Accepted,
    Rejected { failure_count: usize },
}

/// Configuration for the gatekeeper.
#[derive(Debug, Clone)]
pub struct GatekeeperConfig {
    /// Maximum allowed failure rate (0.0 to 1.0). Default: 0.0 (any failure rejects).
    pub max_failure_rate: f64,
    /// Maximum absolute failures allowed. Default: 0.
    pub max_failures: usize,
}

impl Default for GatekeeperConfig {
    fn default() -> Self {
        Self { max_failure_rate: 0.0, max_failures: 0 }
    }
}

/// The gatekeeper that makes the accept/reject decision.
pub struct Gatekeeper {
    config: GatekeeperConfig,
}

impl Gatekeeper {
    pub fn new(config: GatekeeperConfig) -> Self {
        Self { config }
    }

    /// Evaluate a regression report and return a decision.
    pub fn evaluate(&self, report: &RegressionReport) -> GatekeeperDecision {
        if report.failures.len() > self.config.max_failures {
            return GatekeeperDecision::Rejected { failure_count: report.failures.len() };
        }
        if report.pass_rate() < 1.0 - self.config.max_failure_rate {
            return GatekeeperDecision::Rejected { failure_count: report.failures.len() };
        }
        GatekeeperDecision::Accepted
    }
}

impl Default for Gatekeeper {
    fn default() -> Self {
        Self::new(GatekeeperConfig::default())
    }
}

// ─── Dual-Execution Validator ────────────────────────────────────────────────

/// Validates that a compiled function matches simulation output.
pub struct DualExecutionValidator;

impl DualExecutionValidator {
    /// Validate that a compiled function produces the same output as simulation.
    pub fn validate<F>(
        compiled_fn: F,
        transactions: &[SimTransaction],
    ) -> Result<(), Vec<(usize, String, String)>>
    where
        F: Fn(&str) -> String,
    {
        let mut mismatches = Vec::new();

        for (idx, tx) in transactions.iter().enumerate() {
            let binary_output = compiled_fn(&tx.input_event);
            if binary_output.trim() != tx.simulated_output.trim() {
                mismatches.push((idx, tx.simulated_output.clone(), binary_output));
            }
        }

        if mismatches.is_empty() {
            Ok(())
        } else {
            Err(mismatches)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_history_auto_passes() {
        let runner = DifferentialRegressionRunner::new();
        let report = runner.run(|_| "anything".to_string());
        assert!(report.is_clean());
        assert_eq!(report.total_assertions, 0);
    }

    #[test]
    fn test_single_ledger_pass() {
        let mut runner = DifferentialRegressionRunner::new();
        runner.add_ledger("test_ledger", vec![
            SimTransaction::new("GET /api/test", r#"{"status": 200}"#),
            SimTransaction::new("POST /api/action", r#"{"done": true}"#),
        ]);
        let proposed = HashMap::from([
            ("GET /api/test".to_string(), r#"{"status": 200}"#.to_string()),
            ("POST /api/action".to_string(), r#"{"done": true}"#.to_string()),
        ]);
        let report = runner.run_against_map(&proposed);
        assert!(report.is_clean());
        assert_eq!(report.passed, 2);
    }

    #[test]
    fn test_single_ledger_fail() {
        let mut runner = DifferentialRegressionRunner::new();
        runner.add_ledger("api_ledger", vec![
            SimTransaction::new("GET /api/test", r#"{"status": 200}"#),
        ]);
        let proposed = HashMap::from([
            ("GET /api/test".to_string(), r#"{"status": 500}"#.to_string()),
        ]);
        let report = runner.run_against_map(&proposed);
        assert!(!report.is_clean());
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].step_index, 0);
    }

    #[test]
    fn test_multiple_ledgers() {
        let mut runner = DifferentialRegressionRunner::new();
        runner.add_ledger("ledger_a", vec![
            SimTransaction::new("event_1", "output_1"),
        ]);
        runner.add_ledger("ledger_b", vec![
            SimTransaction::new("event_2", "output_2"),
            SimTransaction::new("event_3", "output_3"),
        ]);
        assert_eq!(runner.ledger_count(), 2);
        assert_eq!(runner.total_transactions(), 3);
    }

    #[test]
    fn test_mixed_results() {
        let mut runner = DifferentialRegressionRunner::new();
        runner.add_ledger("mixed", vec![
            SimTransaction::new("pass_event", "expected"),
            SimTransaction::new("fail_event", "expected"),
        ]);
        let report = runner.run(|input| {
            if input == "pass_event" { "expected".to_string() }
            else { "different".to_string() }
        });
        assert_eq!(report.passed, 1);
        assert_eq!(report.failed, 1);
    }

    #[test]
    fn test_gatekeeper_accepts_clean() {
        let runner = DifferentialRegressionRunner::new();
        let report = runner.run(|_| "ok".to_string());
        let gatekeeper = Gatekeeper::default();
        assert_eq!(gatekeeper.evaluate(&report), GatekeeperDecision::Accepted);
    }

    #[test]
    fn test_gatekeeper_rejects_dirty() {
        let mut runner = DifferentialRegressionRunner::new();
        runner.add_ledger("test", vec![
            SimTransaction::new("e1", "expected"),
        ]);
        let report = runner.run(|_| "wrong".to_string());
        let gatekeeper = Gatekeeper::default();
        match gatekeeper.evaluate(&report) {
            GatekeeperDecision::Rejected { failure_count } => assert_eq!(failure_count, 1),
            _ => panic!("Expected rejection"),
        }
    }

    #[test]
    fn test_gatekeeper_tolerance() {
        let mut runner = DifferentialRegressionRunner::new();
        runner.add_ledger("test", vec![
            SimTransaction::new("e1", "ok"),
            SimTransaction::new("e2", "ok"),
            SimTransaction::new("e3", "ok"),
            SimTransaction::new("e4", "wrong"),
        ]);
        let report = runner.run(|input| {
            if input == "e4" { "different".to_string() } else { "ok".to_string() }
        });
        // 75% pass rate, 1 failure
        let gatekeeper = Gatekeeper::new(GatekeeperConfig {
            max_failure_rate: 0.3,
            max_failures: 1,
        });
        assert_eq!(gatekeeper.evaluate(&report), GatekeeperDecision::Accepted);
    }

    #[test]
    fn test_dual_execution_pass() {
        let transactions = vec![
            SimTransaction::new("add 1 2", "3"),
            SimTransaction::new("add 3 4", "7"),
        ];
        let result = DualExecutionValidator::validate(
            |input| {
                let parts: Vec<&str> = input.split_whitespace().collect();
                if parts.len() == 3 && parts[0] == "add" {
                    let a: i32 = parts[1].parse().unwrap();
                    let b: i32 = parts[2].parse().unwrap();
                    (a + b).to_string()
                } else { "error".to_string() }
            },
            &transactions,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_dual_execution_fail() {
        let transactions = vec![
            SimTransaction::new("compute", "42"),
        ];
        let result = DualExecutionValidator::validate(
            |_| "7".to_string(),
            &transactions,
        );
        assert!(result.is_err());
        let errs = result.unwrap_err();
        assert_eq!(errs.len(), 1);
    }

    #[test]
    fn test_report_display() {
        let report = RegressionReport {
            total_ledgers: 2, total_assertions: 10, passed: 8, failed: 2,
            failures: vec![],
        };
        let display = format!("{}", report);
        assert!(display.contains("80.0%"));
    }

    #[test]
    fn test_serialization() {
        let mut runner = DifferentialRegressionRunner::new();
        runner.add_ledger("test", vec![
            SimTransaction::new("input", "output"),
        ]);
        let report = runner.run(|_| "output".to_string());
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("total_ledgers"));
    }
}
