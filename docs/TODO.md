# Implementation TODO — Toy Payments Engine

> **Status: complete.** All phases shipped; kept as a historical record of
> the implementation order.

Source of truth: `docs/superpowers/specs/2026-07-31-payments-engine-design.md`.
Order follows TDD: within each phase, write the failing test first, then the code that passes it.
Each phase ends green (`cargo test` passes) and gets its own commit.

---

## Phase 0 — Project scaffold

- [ x ] `git init` in the project folder
- [ x ] `cargo init --name payments_engine` (binary + lib layout comes next)
- [ x ] Add dependencies to `Cargo.toml`:
  - [ x ] `csv`
  - [ x ] `serde` (features: `derive`)
  - [ x ] `rust_decimal` (features: `serde`)
  - [ x ] `thiserror`
  - [ x ] `anyhow`
  - [ x ] dev-dependency: `rust_decimal_macros`
- [ x ] Create `src/lib.rs` declaring modules: `model`, `store`, `engine`, `io` (empty files for now)
- [ x ] Reduce `src/main.rs` to a stub that calls into the lib
- [ x ] `.gitignore` (`/target`)
- [ x ] `cargo build` passes
- [ x ] Commit: scaffold + spec + transcripts (first commit includes the design doc)

## Phase 1 — Domain model (`model.rs`)

- [ x ] Type aliases: `pub type ClientId = u16;`, `pub type TxId = u32;`
- [ x ] `Transaction` enum (Deposit/Withdrawal with `amount: Decimal`; Dispute/Resolve/Chargeback without)
- [ x ] `Account { available, held, locked }` with `Default` (zeros, unlocked)
- [ x ] `Account::total()` helper → `available + held` (computed, never stored)
- [ x ] `DepositState { Posted, Disputed, ChargedBack }`
- [ x ] `DepositRecord { client, amount, state }`
- [ x ] `TxError` enum with `thiserror` — variants: `AccountLocked`, `NonPositiveAmount`, `DuplicateTx`, `InsufficientFunds`, `UnknownTx`, `ClientMismatch`, `NotDisputable`, `NotUnderDispute` (derive `Debug, PartialEq`)
- [ x ] `cargo build` passes; commit

## Phase 2 — Store (`store.rs`)

- [ x ] Test: `account_or_create` creates a default account on first access, returns the same account after
- [ x ] Test: `insert_deposit` + `get_deposit` round-trip; `get_deposit` on unknown id → `None`
- [ x ] Test: `contains_tx` true after insert, false before
- [ x ] Implement `Store { accounts: HashMap<ClientId, Account>, deposits: HashMap<TxId, DepositRecord> }`
- [ x ] Methods: `account_or_create(&mut, ClientId) -> &mut Account`, `get_account`, `insert_deposit`, `get_deposit`, `get_deposit_mut`, `contains_tx`, `iter_accounts()` (for output)
- [ x ] `cargo test` green; commit

## Phase 3 — Engine rules (`engine.rs`) — one test per rules-table cell

`PaymentEngine` owns a `Store`; `fn process(&mut self, tx: Transaction) -> Result<(), TxError>`.
Every rejection test also asserts **state is untouched** (balances, deposit state, lock flag).
Use `rust_decimal_macros::dec!` in tests.

### 3a. Deposit
- [ x ] Test: happy path → `available += amount`, record stored as `Posted`
- [ x ] Test: locked account → `Err(AccountLocked)`
- [ x ] Test: amount == 0 and amount < 0 → `Err(NonPositiveAmount)`
- [ x ] Test: duplicate tx id → `Err(DuplicateTx)`, first deposit unchanged (first wins)
- [ x ] Test: rejected deposit still creates the account (ghost client, decision #8)
- [ x ] Test: check order — locked account + negative amount → `AccountLocked` (locked checked first)
- [ x ] Implement deposit branch

### 3b. Withdrawal
- [ x ] Test: happy path → `available -= amount`; withdrawal is NOT stored in deposits map
- [ x ] Test: locked → `Err(AccountLocked)`
- [ x ] Test: amount ≤ 0 → `Err(NonPositiveAmount)`
- [ x ] Test: `available < amount` → `Err(InsufficientFunds)`, balance unchanged
- [ x ] Test: withdrawal exactly equal to available succeeds (boundary)
- [ x ] Test: rejected withdrawal still creates the account (ghost client)
- [ x ] Implement withdrawal branch

### 3c. Dispute
- [ x ] Test: happy path → `available -= amt`, `held += amt`, state → `Disputed`
- [ x] Test: unknown tx id → `Err(UnknownTx)`
- [x ] Test: dispute referencing a withdrawal's id → `Err(UnknownTx)` (withdrawals not stored)
- [x ] Test: client mismatch → `Err(ClientMismatch)`
- [x ] Test: already `Disputed` → `Err(NotDisputable)` (double dispute)
- [x ] Test: `ChargedBack` → `Err(NotDisputable)` (terminal)
- [x ] Test: dispute after withdrawal drives `available` negative, `total` still correct (fraud case)
- [x ] Test: dispute on a locked account still processes (locked only blocks deposit/withdrawal)
- [x ] Test: check order — unknown tx cannot yield `ClientMismatch` (`UnknownTx` first)
- [x ] Implement dispute branch

### 3d. Resolve
- [x ] Test: happy path → `held -= amt`, `available += amt`, state back to `Posted`
- [x ] Test: unknown tx → `Err(UnknownTx)`
- [x ] Test: client mismatch → `Err(ClientMismatch)`
- [x ] Test: state `Posted` (no dispute open) → `Err(NotUnderDispute)`
- [x ] Test: state `ChargedBack` → `Err(NotUnderDispute)`
- [x ] Test: re-dispute after resolve is allowed (dispute → resolve → dispute succeeds)
- [x ] Test: resolve on a locked account still processes
- [x ] Implement resolve branch

### 3e. Chargeback
- [x ] Test: happy path → `held -= amt`, state → `ChargedBack`, `locked = true`
- [x ] Test: unknown tx → `Err(UnknownTx)`
- [x ] Test: client mismatch → `Err(ClientMismatch)`
- [x ] Test: state `Posted` → `Err(NotUnderDispute)`; state `ChargedBack` → `Err(NotUnderDispute)`
- [x ] Test: after chargeback, new deposit/withdrawal on that client → `Err(AccountLocked)`
- [x ] Test: after chargeback, dispute on a DIFFERENT posted deposit of the same client still processes
- [x ] Implement chargeback branch
- [x ] `cargo test` green; commit

## Phase 4 — CSV I/O (`io.rs`)

### 4a. Reading / parse boundary
- [ x ] `CsvRecord { type: String, client, tx, amount: Option<Decimal> }` (serde; `type` needs `#[serde(rename)]` — `type` is a keyword)
- [ x] Test: `CsvRecord → Transaction` conversion — all five type strings map correctly
- [ x] Test: unknown type string → conversion error
- [ x] Test: missing amount on deposit/withdrawal → conversion error
- [ x] Test: amount present on dispute/resolve/chargeback → tolerated and dropped
- [ x] Test: amount with > 4 decimal places → rounded to 4 dp (`round_dp(4)`) at conversion
- [ x] Reader builder: `trim(Trim::All)`, `flexible(true)`, streaming (`deserialize()` iterator — never load the file)
- [ x] Test: whitespace-padded row (`deposit, 1, 1, 1.0`) parses
- [ x] Reader yields per-row `Result` so `main` can log parse failures and continue

### 4b. Writing
- [ x ] Test: output has header `client,available,held,total,locked`
- [ x ] Test: one row per account; `total = available + held` computed at write time; `locked` printed `true`/`false`; decimals printed as-is
- [ x ] `write_accounts<W: Write>(store, writer)` — generic over writer so tests use `Vec<u8>`
- [ x ] `cargo test` green; commit

## Phase 5 — CLI (`main.rs`)

- [ x] Read file path from `std::env::args` — missing arg → `anyhow` error, nonzero exit
- [ x] Unreadable/missing file → `anyhow` context, nonzero exit
- [ x] Wire: stream rows → convert → `engine.process()`; on `Err` (parse or `TxError`) log to `eprintln!` and continue
- [ x] Write final accounts CSV to stdout
- [ x] Manual check: `cargo run -- transactions.csv > accounts.csv` with the spec's 5-row example — stdout clean, rejects on stderr only
- [ x] Commit

## Phase 6 — Integration tests (`tests/`)

- [x] Helper: run a CSV string through the full pipeline, parse output into `HashMap<ClientId, Row>` — **order-insensitive** comparison (decision #9)
- [x] Scenario 1: spec's five-row example → exact expected balances
- [x] Scenario 2: fraud flow — deposit → withdrawal → dispute deposit → chargeback (negative available, locked, total reflects loss)
- [x] Scenario 3: dispute → resolve → re-dispute → chargeback
- [x] Scenario 4: whitespace-heavy input
- [x] Scenario 5: malformed rows mixed in (bad type, missing amount, junk line) — engine survives, output correct
- [x] Scenario 6: locked account — subsequent deposit/withdrawal ignored, dispute on older deposit still processes
- [x] Scenario 7: every ignore rule end-to-end (unknown tx, client mismatch, double dispute, resolve without dispute, chargeback after resolve, zero/negative amounts, duplicate deposit id)
- [x] `cargo test` green; commit

## Phase 7 — README + polish

- [x] README — assumptions section: disputes on deposits only; negative available allowed (fraud debt); locked semantics; ghost clients included; withdrawal-id uniqueness trusted (duplicates would be balance-checked, cannot overdraw); tx ids unique among deposits/withdrawals only
- [x] README — scalability: streamed input, O(#clients + #deposits) memory, u32 bounds the tx universe
- [x] README — production sketch: Postgres append-only double-entry ledger, `UNIQUE(tx_id)`, WHERE-guarded transitions, `SELECT … FOR UPDATE` per-client locks
- [x] README — TCP-streams answer: per-client ordering / shard by client id in front of the same synchronous `process(tx)` core
- [x] README — how to run + test
- [x] `cargo fmt` + `cargo clippy` clean (fix warnings)
- [x] Full `cargo test` + one final manual end-to-end run
- [x] Final review pass against spec sections 5 (rules table) and 11 (out of scope — confirm nothing crept in)
- [x] Commit

---

## Guardrails while implementing (from the spec — do not drift)

- No floats anywhere; `Decimal` only.
- Engine performs no I/O; all logging lives in `main`.
- State is never mutated on any `Err` path.
- Check order per operation is fixed as listed in the rules table.
- No sorting of output; no storage trait; no async; no withdrawal-id tracking.

