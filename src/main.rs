use anyhow::Context;
use payment_engine::engine::PaymentEngine;
use payment_engine::io::{process_csv, write_accounts};
use std::fs::File;
use std::io::{BufWriter, Write};

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .context("usage: payment_engine <transactions.csv>")?;

    let file = File::open(&path).with_context(|| format!("cannot open input file {path:?}"))?;

    let mut engine = PaymentEngine::new();
    process_csv(file, &mut engine, |error| eprintln!("{error}"));

    let mut writer = BufWriter::new(std::io::stdout().lock());
    write_accounts(engine.store(), &mut writer)?;
    writer.flush().context("cannot write accounts output")?;

    Ok(())
}
