# Toy Payments Engine — Design Spec

**Status:** Approved for implementation
**Run contract:** `cargo run -- transactions.csv > accounts.csv`

## 1. Purpose

A single-threaded payments engine that streams a chronological CSV of transactions
(deposits, withdrawals, disputes, resolves, chargebacks), maintains per-client account
state, and writes the final account states as CSV to stdout. Diagnostics go to stderr —
stdout carries only the output CSV.

## 2. Decisions (all discussed and locked)

| # | Decision | Choice |
|---|----------|--------|
| 1 | Dispute scope | **Deposits only.** Disputes referencing a withdrawal are ignored (`UnknownTx` — withdrawals are not stored). Documented as a README assumption. |
| 2 | Money type | **`rust_decimal::Decimal`** (serde feature). No floats anywhere. |
| 3 | Storage | **`Store` struct, no trait.** Two HashMaps behind intention-revealing methods. Postgres/ledger story lives in the README only. |
| 4 | Malformed rows | **Log to stderr + skip**, processing continues. |
| 5 | Duplicate deposit tx id | **First wins**; the duplicate is rejected (`DuplicateTx`) and logged. |
| 6 | Duplicate withdrawal tx id | **Not tracked.** Spec guarantees uniqueness; a duplicate would still be balance-checked and cannot overdraw. README line. (Bloom filter explicitly rejected: false positives would reject legitimate withdrawals.) |
| 7 | > 4 decimal places in input | **Round to 4 dp at ingest** (`round_dp`, at CSV→domain conversion). |
| 8 | Ghost clients | **Any well-formed deposit/withdrawal row creates the account**, even if the operation is then rejected. Appears in output with zero balances. |
| 9 | Output row order | **Unsorted** (HashMap iteration order). Integration tests compare order-insensitively. |
| 10 | Output decimal format | **Print `Decimal` as-is.** Scale ≤ 4 is guaranteed by ingest rounding; mixed scales (`1.5`, `3.0`) are acceptable. |
| 11 | Rejection reporting | Engine returns **`Result<(), TxError>` per transaction**; `main` logs rejections to stderr. Engine performs no I/O. |
| 12 | Error tooling | **`thiserror`** for `TxError`, **`anyhow`** in `main` only. |
| 13 | Crate layout | **`lib.rs` + thin `main.rs`**; integration tests call the library. |
| 14 | Development style | **TDD** — tests written before each rule's implementation. |

## 3. Architecture

```
main.rs (CLI: parse arg, wire pipeline, log rejections to stderr, exit code)
  └─> io::read — csv::Reader streaming row by row (file is never fully loaded)
        └─> per row: CsvRecord → Transaction conversion (parse-level hygiene)
              └─> engine.process(tx) -> Result<(), TxError>
  └─> io::write — iterate accounts, compute total, write CSV to stdout
```

Modules in `lib.rs`: `model`, `store`, `engine`, `io`.

**Memory:** O(#clients + #deposits). Withdrawals mutate balances and are not retained.
**CSV reader config:** `trim(Trim::All)` (tolerates `deposit, 1, 1, 1.0` spacing),
`flexible(true)` (tolerates missing trailing amount column on dispute rows).

## 4. Data model (`model.rs`)

```rust
pub type ClientId = u16;
pub type TxId = u32;

// serde target for csv; tolerant by construction
struct CsvRecord { type: String, client: ClientId, tx: TxId, amount: Option<Decimal> }

pub enum Transaction {
    Deposit    { client: ClientId, tx: TxId, amount: Decimal },
    Withdrawal { client: ClientId, tx: TxId, amount: Decimal },
    Dispute    { client: ClientId, tx: TxId },
    Resolve    { client: ClientId, tx: TxId },
    Chargeback { client: ClientId, tx: TxId },
}

pub struct Account { pub available: Decimal, pub held: Decimal, pub locked: bool }
// `total` is computed at output time (available + held) — never stored, cannot drift.

pub struct DepositRecord { pub client: ClientId, pub amount: Decimal, pub state: DepositState }
pub enum DepositState { Posted, Disputed, ChargedBack }
```

`CsvRecord → Transaction` conversion rules (this is the parse boundary):

- Unknown `type` string → parse error (skip + stderr).
- Missing amount on deposit/withdrawal → parse error.
- Amount present on dispute/resolve/chargeback → tolerated, dropped.
- Amount rounded to 4 dp here (`round_dp(4)`), before the engine ever sees it.

## 5. Engine rules (`engine.rs`)

`PaymentEngine` owns a `Store`. `fn process(&mut self, tx: Transaction) -> Result<(), TxError>`.
State is never mutated on any `Err` path.

| Tx | Rejected when → `TxError` | Effect when accepted |
|---|---|---|
| Deposit | account locked → `AccountLocked` · amount ≤ 0 → `NonPositiveAmount` · tx id already stored → `DuplicateTx` | `available += amt`; store `DepositRecord { Posted }` |
| Withdrawal | locked → `AccountLocked` · amount ≤ 0 → `NonPositiveAmount` · `available < amt` → `InsufficientFunds` | `available -= amt` |
| Dispute | tx not stored → `UnknownTx` · client ≠ record.client → `ClientMismatch` · state ≠ `Posted` → `NotDisputable` | `available -= amt; held += amt`; state → `Disputed` |
| Resolve | `UnknownTx` · `ClientMismatch` · state ≠ `Disputed` → `NotUnderDispute` | `held -= amt; available += amt`; state → `Posted` |
| Chargeback | `UnknownTx` · `ClientMismatch` · state ≠ `Disputed` → `NotUnderDispute` | `held -= amt`; state → `ChargedBack`; `locked = true` |

Semantics locked in:

- **Locked** blocks only new deposits/withdrawals. Disputes, resolves and chargebacks
  on pre-existing transactions still process.
- **Available may go negative** via dispute-after-withdrawal (deposit 5, withdraw 5,
  dispute the deposit → available −5, held 5, total 0). Deliberate: it is the client's
  debt in the fraud scenario. No `available ≥ 0` invariant exists.
- **Held is never negative** (only ever increased by a dispute and decreased by the
  matching resolve/chargeback of the same amount, guarded by `DepositState`).
- **Re-dispute after resolve is allowed** (`Resolve` returns state to `Posted`).
- **Chargeback is terminal** for that deposit (`ChargedBack` is never disputable again).
- **Account creation:** deposit/withdrawal use `entry(client).or_default()` before rule
  checks (ghost clients, decision #8). Dispute/resolve/chargeback never create accounts —
  they require a stored deposit, whose owner's account necessarily exists.
- Order of checks is as listed per row of the table (e.g. locked is checked before
  amount validation).

## 6. Store (`store.rs`)

```rust
pub struct Store {
    accounts: HashMap<ClientId, Account>,
    deposits: HashMap<TxId, DepositRecord>,
}
```

Methods are intention-revealing (`account_or_create`, `get_deposit`, `insert_deposit`, …)
so the engine reads as pure business rules. No storage trait — swapping in a real
database is a one-module rewrite done with real requirements in hand (see README notes).

## 7. Error handling

```rust
#[derive(thiserror::Error, Debug, PartialEq)]
pub enum TxError {
    AccountLocked, NonPositiveAmount, DuplicateTx,
    InsufficientFunds, UnknownTx, ClientMismatch,
    NotDisputable, NotUnderDispute,
}
```

- Every "ignore" rule from the requirements is a named, testable variant.
- Parse failures (malformed CSV rows, unknown type strings) are a separate error at the
  io/conversion layer — also logged to stderr and skipped.
- `main` uses `anyhow` for CLI-edge context (missing file argument, unreadable file).
- Rejections and parse skips are **not** fatal: the run always produces output unless the
  input file itself cannot be opened.

## 8. Output (`io.rs`)

- Header `client,available,held,total,locked`.
- One row per account in map iteration order (unsorted, decision #9).
- `total = available + held` computed at write time.
- Decimals printed as-is (decision #10); `locked` as `true`/`false`.

## 9. Testing strategy (TDD)

**Unit tests** (co-located `#[cfg(test)]` in `engine.rs`): one test per cell of the rules
table — every happy path and every rejection variant, asserting the exact `TxError` and
that state is untouched on rejection. `Decimal` literals via `rust_decimal_macros::dec!`.

**Integration tests** (`tests/`): CSV string → full pipeline → final accounts compared
**order-insensitively** (parse output rows into a map keyed by client id). Scenarios:

1. The spec's five-row example → exact expected balances.
2. Fraud flow: deposit → withdrawal → dispute of the deposit → chargeback
   (negative available, then locked, total reflects the loss).
3. Dispute → resolve → re-dispute → chargeback.
4. Whitespace-heavy input (`deposit, 1, 1, 1.0`).
5. Malformed rows mixed in (bad type, missing amount, junk) — engine survives, output correct.
6. Locked account: subsequent deposit/withdrawal ignored, dispute on an older deposit still processes.
7. Every ignore rule end-to-end (unknown tx, client mismatch, double dispute, resolve
   without dispute, chargeback after resolve, zero/negative amounts, duplicate deposit id).

**Dependencies:** `csv`, `serde` (derive), `rust_decimal` (serde feature), `thiserror`,
`anyhow`; dev: `rust_decimal_macros`.

## 10. README (part of the deliverable)

Must contain:

- Assumptions: disputes on deposits only; negative available allowed (fraud debt); locked
  semantics; ghost clients included; withdrawal-id uniqueness trusted per spec (duplicates
  would each be balance-checked and cannot overdraw); tx ids unique among
  deposits/withdrawals only — dispute-family rows reference existing ids.
- Scalability: streamed input, O(#clients + #deposits) memory; u32 id space bounds the tx
  universe; at real scale this state is a database, not a HashMap.
- Production sketch: state becomes a Postgres append-only double-entry ledger;
  idempotency via constraints (`UNIQUE(tx_id)`, WHERE-guarded state transitions);
  per-client serialization maps to row-level locks (`SELECT … FOR UPDATE`).
- TCP-streams answer: the engine core is a synchronous `process(tx)`; with thousands of
  concurrent streams, global chronological order is replaced by **per-client ordering**
  (shard by client id — clients are independent given the no-transfers assumption),
  e.g. per-client channels/actors in front of the same engine logic.

## 11. Out of scope

- No storage trait, no async, no database, no Bloom/Roaring id tracking.
- No sorting of output; no fixed-width decimal formatting.
- No transfers between clients (each tx id belongs to exactly one client's interaction
  with the outside world).
