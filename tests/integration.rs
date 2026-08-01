use payment_engine::engine::PaymentEngine;
use payment_engine::io::{process_csv, write_accounts};
use payment_engine::model::ClientId;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;

#[derive(Debug, PartialEq)]
struct Row {
    available: Decimal,
    held: Decimal,
    total: Decimal,
    locked: bool,
}

fn row(available: Decimal, held: Decimal, locked: bool) -> Row {
    Row { available, held, total: available + held, locked }
}

/// Resolves a fixture file name to its absolute path so tests work
/// regardless of the current working directory.
fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures")).join(name)
}

/// Runs a CSV fixture file through the full pipeline (parse -> engine -> writer)
/// and returns the output keyed by client id (order-insensitive by construction)
/// plus every error the pipeline reported along the way.
fn run_pipeline(fixture: &str) -> (HashMap<ClientId, Row>, Vec<String>) {
    let mut engine = PaymentEngine::new();
    let mut errors = Vec::new();
    let file = File::open(fixture_path(fixture)).unwrap();
    process_csv(file, &mut engine, |e| errors.push(e));

    let mut out = Vec::new();
    write_accounts(engine.store(), &mut out).unwrap();

    (parse_output(&String::from_utf8(out).unwrap()), errors)
}

fn parse_output(output: &str) -> HashMap<ClientId, Row> {
    let mut reader = csv::Reader::from_reader(output.as_bytes());
    assert_eq!(
        reader.headers().unwrap(),
        &csv::StringRecord::from(vec!["client", "available", "held", "total", "locked"]),
        "output must start with the header"
    );

    let mut rows = HashMap::new();
    for record in reader.deserialize() {
        let (client, available, held, total, locked): (ClientId, Decimal, Decimal, Decimal, bool) =
            record.unwrap();

        let previous = rows.insert(client, Row { available, held, total, locked });
        assert!(previous.is_none(), "client {client} appears twice in output");
    }
    rows
}

#[test]
fn scenario_1_spec_example() {
    let (accounts, _) = run_pipeline("spec_example.csv");

    assert_eq!(
        accounts,
        HashMap::from([
            (1, row(dec!(1.5), dec!(0), false)),
            (2, row(dec!(2), dec!(0), false)),
        ])
    );
}

#[test]
fn scenario_2_fraud_flow_leaves_negative_available_and_locks() {
    let (accounts, _) = run_pipeline("fraud_flow.csv");

    assert_eq!(
        accounts,
        HashMap::from([(1, row(dec!(-5), dec!(0), true))])
    );
}

#[test]
fn scenario_3_dispute_resolve_redispute_chargeback() {
    let (accounts, _) = run_pipeline("dispute_resolve_redispute_chargeback.csv");

    assert_eq!(
        accounts,
        HashMap::from([(1, row(dec!(0), dec!(0), true))])
    );
}

#[test]
fn scenario_4_whitespace_heavy_input_and_short_dispute_row() {
    // The fixture has mixed spacing everywhere, plus a dispute row with only
    // 3 columns (exercises flexible(true) end-to-end).
    let (accounts, _) = run_pipeline("whitespace_heavy.csv");

    assert_eq!(
        accounts,
        HashMap::from([(1, row(dec!(-0.5), dec!(2), false))])
    );
}

#[test]
fn scenario_5_malformed_rows_are_skipped_and_reported() {
    let (accounts, errors) = run_pipeline("malformed_rows.csv");

    assert_eq!(
        accounts,
        HashMap::from([
            (1, row(dec!(1), dec!(0), false)),
            (2, row(dec!(2), dec!(0), false)),
        ])
    );
    // unknown type, missing amount, junk line, unparseable client id
    assert_eq!(errors.len(), 4, "expected 4 skipped rows, got: {errors:?}");
}

#[test]
fn scenario_6_locked_account_blocks_new_funds_but_disputes_still_process() {
    let (accounts, _) = run_pipeline("locked_account.csv");

    // After chargeback of tx 1: available 3, locked.
    // deposit tx 3 / withdrawal tx 4 ignored (locked).
    // dispute of tx 2 still processes: available 0, held 3.
    assert_eq!(
        accounts,
        HashMap::from([(1, row(dec!(0), dec!(3), true))])
    );
}

#[test]
fn scenario_7_every_ignore_rule_end_to_end() {
    // Client 1 walks through every dispute-family ignore rule; the deposit
    // amount 5.00001 must be rounded to 5.0000 at ingest or the final
    // balance drifts (rounding visible end-to-end).
    // Clients 3, 4, 5 are ghost clients: their only row is rejected but the
    // account must still appear with zero balances.
    let (accounts, _) = run_pipeline("ignore_rules.csv");

    // Client 1 timeline:
    //   deposit 5.0000 (rounded), withdraw 1.0          -> available 4
    //   dispute tx 99        -> ignored (unknown tx)
    //   dispute by client 2  -> ignored (client mismatch)
    //   dispute tx 2         -> ignored (withdrawal id, not stored)
    //   dispute tx 1         -> available -1, held 5
    //   dispute tx 1 again   -> ignored (double dispute)
    //   resolve tx 1         -> available 4, held 0
    //   chargeback tx 1      -> ignored (chargeback after resolve)
    //   resolve tx 1         -> ignored (resolve without dispute)
    //   dispute tx 1         -> re-dispute allowed: available -1, held 5
    //   chargeback tx 1      -> held 0, locked
    //   chargeback tx 1      -> ignored (already charged back)
    //   resolve tx 1         -> ignored (already charged back)
    // Client 5's deposit reuses tx id 1 -> ignored (duplicate, first wins).
    assert_eq!(
        accounts,
        HashMap::from([
            (1, row(dec!(-1), dec!(0), true)),
            (3, row(dec!(0), dec!(0), false)),
            (4, row(dec!(0), dec!(0), false)),
            (5, row(dec!(0), dec!(0), false)),
        ])
    );
}

#[test]
fn header_only_input_produces_header_only_output() {
    let (accounts, _) = run_pipeline("header_only.csv");

    assert_eq!(accounts, HashMap::new());
}

#[test]
fn empty_input_produces_header_only_output() {
    let (accounts, _) = run_pipeline("empty.csv");

    assert_eq!(accounts, HashMap::new());
}
