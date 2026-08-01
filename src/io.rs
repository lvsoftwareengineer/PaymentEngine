use anyhow::Error;
use rust_decimal::Decimal;
use crate::model::{ClientId, TxId, Transaction};
use crate::store::Store;
use csv::{ReaderBuilder, Trim};
use serde::Deserialize;
use std::io::{Read, Write};
use thiserror::Error;

impl TryFrom<CsvRecord> for Transaction {
    type Error = RowError;

    fn try_from(record: CsvRecord) -> Result<Self, Self::Error> {
        let CsvRecord { kind, client, tx, amount } = record;

        match kind.to_ascii_lowercase().as_str() {
            "deposit" => Ok(Transaction::Deposit {
                client,
                tx,
                amount: amount.ok_or(RowError::MissingAmount { tx })?.round_dp(4),
            }),
            "withdrawal" => Ok(Transaction::Withdraw {
                client,
                tx,
                amount: amount.ok_or(RowError::MissingAmount { tx })?.round_dp(4),
            }),
            "dispute" => Ok(Transaction::Dispute { client, tx }),
            "resolve" => Ok(Transaction::Resolve { client, tx }),
            "chargeback" => Ok(Transaction::Chargeback { client, tx }),
            other => Err(RowError::UnknownType { kind: other.to_string(), tx }),
        }
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum RowError {
    #[error("unknown transaction type {kind:?} (tx {tx})")]
    UnknownType { kind: String, tx: TxId },

    #[error("tx {tx}: this transaction type requires an amount")]
    MissingAmount { tx: TxId },
}

pub fn csv_reader<R: Read>(source: R) -> csv::Reader<R> {
    ReaderBuilder::new()
        .trim(Trim::All)
        .flexible(true)
        .from_reader(source)
}

pub fn write_accounts<W: Write>(store: &Store, mut writer: W) -> Result<(), Error> {
    writeln!(writer, "client,available,held,total,locked")?;

    for (client, account) in store.iter_accounts() {
        writeln!(
            writer,
            "{},{},{},{},{}",
            client,
            account.available,
            account.held,
            account.total(),
            account.locked
        )?;
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct CsvRecord {
    #[serde(rename = "type")]
    pub kind: String,
    pub client: ClientId,
    pub tx: TxId,
    pub amount: Option<Decimal>
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Transaction;
    use rust_decimal_macros::dec;

    fn record(kind: &str, client: ClientId, tx: TxId, amount: Option<Decimal>) -> CsvRecord {
        CsvRecord { kind: kind.to_string(), client, tx, amount }
    }

    // --- CsvRecord -> Transaction conversion ---

    #[test]
    fn all_five_type_strings_convert_to_the_right_variant() {
        assert_eq!(
            Transaction::try_from(record("deposit", 1, 1, Some(dec!(1.5)))),
            Ok(Transaction::Deposit { client: 1, tx: 1, amount: dec!(1.5) })
        );
        assert_eq!(
            Transaction::try_from(record("withdrawal", 1, 2, Some(dec!(1.5)))),
            Ok(Transaction::Withdraw { client: 1, tx: 2, amount: dec!(1.5) })
        );
        assert_eq!(
            Transaction::try_from(record("dispute", 1, 1, None)),
            Ok(Transaction::Dispute { client: 1, tx: 1 })
        );
        assert_eq!(
            Transaction::try_from(record("resolve", 1, 1, None)),
            Ok(Transaction::Resolve { client: 1, tx: 1 })
        );
        assert_eq!(
            Transaction::try_from(record("chargeback", 1, 1, None)),
            Ok(Transaction::Chargeback { client: 1, tx: 1 })
        );
    }

    #[test]
    fn unknown_type_string_is_a_conversion_error() {
        let result = Transaction::try_from(record("transfer", 1, 1, Some(dec!(1))));

        assert_eq!(
            result,
            Err(RowError::UnknownType { kind: "transfer".to_string(), tx: 1 })
        );
    }

    #[test]
    fn missing_amount_on_deposit_or_withdrawal_is_a_conversion_error() {
        assert_eq!(
            Transaction::try_from(record("deposit", 1, 1, None)),
            Err(RowError::MissingAmount { tx: 1 })
        );
        assert_eq!(
            Transaction::try_from(record("withdrawal", 1, 2, None)),
            Err(RowError::MissingAmount { tx: 2 })
        );
    }

    #[test]
    fn amount_on_dispute_family_rows_is_tolerated_and_dropped() {
        assert_eq!(
            Transaction::try_from(record("dispute", 1, 1, Some(dec!(9)))),
            Ok(Transaction::Dispute { client: 1, tx: 1 })
        );
        assert_eq!(
            Transaction::try_from(record("resolve", 1, 1, Some(dec!(9)))),
            Ok(Transaction::Resolve { client: 1, tx: 1 })
        );
        assert_eq!(
            Transaction::try_from(record("chargeback", 1, 1, Some(dec!(9)))),
            Ok(Transaction::Chargeback { client: 1, tx: 1 })
        );
    }

    #[test]
    fn amount_is_rounded_to_four_decimal_places_at_conversion() {
        assert_eq!(
            Transaction::try_from(record("deposit", 1, 1, Some(dec!(1.23456)))),
            Ok(Transaction::Deposit { client: 1, tx: 1, amount: dec!(1.2346) })
        );
        assert_eq!(
            Transaction::try_from(record("withdrawal", 1, 2, Some(dec!(0.99999)))),
            Ok(Transaction::Withdraw { client: 1, tx: 2, amount: dec!(1.0000) })
        );
    }

    // --- Reader configuration ---

    #[test]
    fn whitespace_padded_rows_parse() {
        let input = "type, client, tx, amount\ndeposit, 1, 1, 1.0\n";

        let mut reader = csv_reader(input.as_bytes());
        let records: Vec<CsvRecord> =
            reader.deserialize().collect::<Result<_, _>>().unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(
            Transaction::try_from(record("deposit", records[0].client, records[0].tx, records[0].amount)),
            Ok(Transaction::Deposit { client: 1, tx: 1, amount: dec!(1.0) })
        );
        assert_eq!(records[0].kind, "deposit");
    }

    #[test]
    fn dispute_row_with_missing_trailing_amount_column_parses() {
        let input = "type, client, tx, amount\ndeposit, 1, 1, 1.0\ndispute, 1, 1\n";

        let mut reader = csv_reader(input.as_bytes());
        let records: Vec<CsvRecord> =
            reader.deserialize().collect::<Result<_, _>>().unwrap();

        assert_eq!(records.len(), 2);
        assert_eq!(records[1].kind, "dispute");
        assert_eq!(records[1].amount, None);
    }
    // --- Output writing ---

    use crate::store::Store;

    fn write_to_string(store: &Store) -> String {
        let mut buf = Vec::new();
        write_accounts(store, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn output_starts_with_the_header() {
        let store = Store::new();

        let output = write_to_string(&store);

        assert_eq!(output.lines().next(), Some("client,available,held,total,locked"));
    }

    #[test]
    fn writes_one_row_per_account_with_computed_total() {
        let mut store = Store::new();

        let a = store.account_or_create(1);
        a.available = dec!(1.5);

        let b = store.account_or_create(2);
        b.available = dec!(-5);
        b.held = dec!(5);
        b.locked = true;

        let output = write_to_string(&store);
        let mut rows: Vec<&str> = output.lines().skip(1).collect();
        rows.sort();

        assert_eq!(rows, vec!["1,1.5,0,1.5,false", "2,-5,5,0,true"]);
    }
}
