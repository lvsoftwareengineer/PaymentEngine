//! A single-threaded toy payments engine.
//!
//! Streams a CSV of transactions (deposits, withdrawals, disputes, resolves,
//! chargebacks) in chronological order, applies them to per-client accounts,
//! and writes the final account states as CSV.
//!
//! # Pipeline
//!
//! [`io::process_csv`] parses each row into a [`model::Transaction`] and feeds
//! it to [`engine::PaymentEngine::process`], which applies every business rule
//! against the state held in [`store::Store`]. [`io::write_accounts`] renders
//! the final balances. Malformed rows and rejected transactions are reported
//! through a callback and skipped — a run always produces output.
//!
//! # Example
//!
//! ```
//! use payment_engine::engine::PaymentEngine;
//! use payment_engine::io::{process_csv, write_accounts};
//!
//! let input = "\
//! type,client,tx,amount
//! deposit,1,1,5.0
//! dispute,1,1
//! chargeback,1,1
//! ";
//!
//! let mut engine = PaymentEngine::new();
//! process_csv(input.as_bytes(), &mut engine, |e| eprintln!("{e}"));
//!
//! let mut out = Vec::new();
//! write_accounts(engine.store(), &mut out).unwrap();
//!
//! // The chargeback drained the disputed funds and locked the account.
//! assert!(String::from_utf8(out).unwrap().contains("1,0,0,0,true"));
//! ```
//!
//! Design rationale, assumptions, and the production sketch live in the
//! repository README.

pub mod engine;
pub mod io;
pub mod model;
pub mod store;
