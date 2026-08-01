use anyhow::Context;
use payment_engine::engine::PaymentEngine;
use payment_engine::io::{csv_reader, write_accounts, CsvRecord};
use payment_engine::model::Transaction;
use std::fs::File;

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .context("usage: payment_engine <transactions.csv>")?;

    let file = File::open(&path)
        .with_context(|| format!("cannot open input file {path:?}"))?;

    let mut engine = PaymentEngine::new();

    for row in csv_reader(file).deserialize::<CsvRecord>() {
        let record = match row {
            Ok(record) => record,
            Err(e) => {
                eprintln!("skipping malformed row: {e}");
                continue;
            }
        };

        match Transaction::try_from(record) {
            Ok(tx) => {
                if let Err(e) = engine.process(tx) {
                    eprintln!("transaction rejected: {e}");
                }
            }
            Err(e) => eprintln!("skipping row: {e}"),
        }
    }

    write_accounts(engine.store(), std::io::stdout().lock())?;

    Ok(())
}
