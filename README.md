# Differential Regression

[![crates.io](https://img.shields.io/crates/v/differential-regression.svg)](https://crates.io/crates/differential-regression)
[![docs.rs](https://docs.rs/differential-regression/badge.svg)](https://docs.rs/differential-regression)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

> **The Gatekeeper — regression testing as a merge gate, with configurable tolerance and dual-execution validation.**

---

## The Problem

You have a behavioral ledger of historical inputs and outputs. Before merging a new specification patch, you need to verify it doesn't break any existing behavior. But you need flexibility — sometimes a 99% pass rate is acceptable, and sometimes only 100% will do.

## Why This Exists

Differential Regression provides:
- **Historical ledger loading** from multiple named ledgers
- **Two-phase regression check**: simulation replay + dual-execution validation
- **Configurable gatekeeper** with tolerance for failure rate and count
- **Dual-execution validation** that verifies compiled code matches simulation
- **Serde-serializable** reports for CI integration

## Architecture

```
  Historical Ledgers ──→ ┌────────────────────┐
                         │  Differential       │
  Proposed Spec ───────→ │  Regression Runner  │
                         │                     │
                         │  simulate_fn(input) │
                         │    → output         │
                         └─────────┬───────────┘
                                   │
                         ┌─────────▼───────────┐
                         │    Gatekeeper        │
                         │  max_failure_rate: 0 │
                         │  max_failures: 0     │
                         │                     │
                         │  ✅ Accepted         │
                         │  ❌ Rejected         │
                         └─────────────────────┘
```

## Installation

```toml
[dependencies]
differential-regression = { version = "0.1", features = ["serde"] }
```

## API Reference

### `DifferentialRegressionRunner`

The core engine for regression checking:

```rust
use differential_regression::*;
use std::collections::HashMap;

let mut runner = DifferentialRegressionRunner::new();
runner.add_ledger("api_tests", vec![
    SimTransaction::new("GET /api/test", r#"{"status": 200}"#),
    SimTransaction::new("POST /api/action", r#"{"done": true}"#),
]);

let proposed = HashMap::from([
    ("GET /api/test".into(), r#"{"status": 200}"#.into()),
    ("POST /api/action".into(), r#"{"done": true}"#.into()),
]);

let report = runner.run_against_map(&proposed);
assert!(report.is_clean());
```

### `Gatekeeper`

Configurable accept/reject decisions:

```rust
use differential_regression::*;

let gatekeeper = Gatekeeper::new(GatekeeperConfig {
    max_failure_rate: 0.0,  // zero tolerance (default)
    max_failures: 0,
});

// Or with tolerance:
let lenient = Gatekeeper::new(GatekeeperConfig {
    max_failure_rate: 0.1,  // allow 10% failures
    max_failures: 5,         // up to 5 individual failures
});
```

### `DualExecutionValidator`

Verify compiled code matches simulation:

```rust
use differential_regression::*;

let transactions = vec![
    SimTransaction::new("add 1 2", "3"),
    SimTransaction::new("add 3 4", "7"),
];

let result = DualExecutionValidator::validate(
    |input| {
        let parts: Vec<&str> = input.split_whitespace().collect();
        let a: i32 = parts[1].parse().unwrap();
        let b: i32 = parts[2].parse().unwrap();
        (a + b).to_string()
    },
    &transactions,
);
assert!(result.is_ok());
```

## Usage Examples

### Example 1: Strict Regression Gate

```rust
use differential_regression::*;

let mut runner = DifferentialRegressionRunner::new();
runner.add_ledger("core", vec![
    SimTransaction::new("event_a", "output_a"),
    SimTransaction::new("event_b", "output_b"),
]);

let report = runner.run(|input| {
    match input {
        "event_a" => "output_a".into(),
        "event_b" => "output_b".into(),
        _ => "unknown".into(),
    }
});

let gatekeeper = Gatekeeper::default();
match gatekeeper.evaluate(&report) {
    GatekeeperDecision::Accepted => println!("✅ Patch approved"),
    GatekeeperDecision::Rejected { failure_count } => 
        println!("❌ {} regressions detected", failure_count),
}
```

### Example 2: Multi-Ledger Regression

```rust
use differential_regression::*;

let mut runner = DifferentialRegressionRunner::new();
runner.add_ledger("api_v1", vec![SimTransaction::new("list", "[1,2,3]")]);
runner.add_ledger("api_v2", vec![SimTransaction::new("list", "[1,2,3,4]")]);

assert_eq!(runner.ledger_count(), 2);
assert_eq!(runner.total_transactions(), 2);
```

### Example 3: CI Integration

```rust
use differential_regression::*;

let report = runner.run(|input| proposed_simulate(input));
println!("{}", report); // "Regression: 2 ledgers, 10 assertions, 8 passed, 2 failed (80.0%)"

if !report.is_clean() {
    for failure in &report.failures {
        eprintln!("{}", failure);
    }
}
```

## Performance

| Operation | Complexity |
|-----------|-----------|
| Run regression | O(L × T × sim_cost) |
| Gatekeeper evaluate | O(1) |
| Dual execution | O(T × fn_cost) |
| Ledger loading | O(L × T) |

## License

Licensed under the [MIT License](LICENSE).

## Contributing

1. Fork the repository
2. Create a feature branch
3. Write tests
4. Push and open a Pull Request
