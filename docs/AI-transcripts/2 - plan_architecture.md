# Session Transcript 2 — Toy Payments Engine: Design Decisions & Spec

Goal of this session: brainstorm every open
design/code decision one at a time, discuss trade-offs, and produce a spec.
**Output:** `docs/superpowers/specs/2026-07-31-payments-engine-design.md` (approved, ready for implementation).

---

## Leonardo

Provided my full problem notes again (goal, requirements, input/output formats,
transaction semantics, assumptions), now with most edge cases from session 1 already
answered inline (locked policy, re-dispute after resolve → allow, resolve/chargeback
without dispute → ignore, client mismatch → ignore, 0/negative amounts → ignore, etc.).

Proposed architecture: a `PaymentEngine` class receiving a Storage type (HashMaps for now —
one for accounts, one for deposit/withdrawal operations), an entrypoint that deserializes
the CSV and feeds the engine, output by iterating the map to stdout at the end, enums for
type safety, TDD for development.

Instruction: brainstorm, ask questions about every code/design decision, point trade-offs,
Leonardo decides. Output is a spec; he reviews it, then clears context and implements.

## Claude

Read `session-transcript.md` to avoid re-litigating settled points. Identified the
remaining open decisions and walked through them one at a time:

### Q1 — Dispute scope: deposits only, or withdrawals too?

Options: deposits only (spec's dispute math is written for deposits; matches the fraud
story; only deposits need storing) · both with literal math (double-punishes the client —
semantically wrong) · both with inverted math for withdrawals (realistic but invents
semantics; forks the state machine).

**Decision: deposits only.** Withdrawals mutate balances and are not retained; disputes
referencing them are ignored. README assumption.

### Q2 — Money representation

`rust_decimal::Decimal` (serde/csv integration out of the box, non-wrapping arithmetic,
idiomatic) vs fixed-point i64 × 10⁴ (zero deps, explicit checked arithmetic, but you own
a custom parser/formatter ~50 lines).

**Decision: `rust_decimal`.**

### Q3 — Storage abstraction

Leonardo asked: what's the downside of a trait? Couldn't we implement it over HashMaps and
leave the door open for Postgres?

Claude's answer: a trait shaped around HashMaps opens the wrong door — Postgres can't
implement it. Three reasons: (1) fallibility — DB calls need `Result` on every method,
HashMaps have no failure mode; (2) ownership — you can't return `&mut Account` borrowed
from a database; the DB shape is fetch → modify → write-back with owned values; (3) async +
transactionality — real drivers ripple `.await` through the engine, and dispute/chargeback
are multi-step invariants needing a unit-of-work API, not per-entity getters (a KV trait
forces the read-modify-write race Postgres exists to prevent). So either the trait mirrors
the HashMap and gets rewritten anyway at migration time (painted-on door), or it's
DB-shaped from day one and the toy pays permanently for a backend that never comes. The
swap-point doesn't need a trait: a `Store` struct in its own module is the same
one-place-to-rewrite, and struct→trait/sync→async is a mechanical refactor done later with
real requirements in hand. A trait's testing benefit is void here — the in-memory store IS
the test double.

**Decision: `Store` struct, no trait.** Intention-revealing methods over two HashMaps;
Postgres story in README only.

### Q4 — Input hygiene

- Malformed rows (unknown type, unparseable numbers, missing amount): **log to stderr +
  skip** (stdout stays clean, keeps a debugging trace).
- Duplicate tx id on deposit: **first wins, ignore the duplicate** (only non-corrupting
  option; consistent with other ignore rules).
- \>4 decimal places: **round to 4 dp at ingest** (`round_dp` at the parse boundary, so
  stored amounts always match output precision).

### Q5 — Output details

- Ghost clients (every tx failed): **yes, any well-formed deposit/withdrawal row creates
  the account** (`entry().or_default()`), appears with zero balances.
- Row ordering — Leonardo challenged the sort suggestion ("we don't need it sorted,
  sorting adds complexity, right?"). Claude clarified: sorting is NOT required for
  correctness; the issue is Rust's HashMap uses a randomly-seeded hasher, so iteration
  order changes every run → whole-file `diff` tests flake. Determinism has to live
  somewhere: sort at output (2 lines) vs order-insensitive test helpers vs BTreeMap
  (pays O(log n) at processing time for an output-time concern — wrong place).
  **Decision: unsorted output; integration tests compare order-insensitively.**
- Decimal formatting: **print `Decimal` as-is** (scale ≤ 4 guaranteed by ingest rounding;
  mixed scales acceptable — both styles are fine, consistency is what matters).

### Q6 — Errors and layout

- Rejection reporting: **engine returns `Result<(), TxError>` per transaction**; `main`
  logs to stderr. Engine stays pure (no I/O in business logic); tests assert the exact
  rejection reason — ideal for TDD on the ignore rules.
- Error tooling: **`thiserror` for `TxError` + `anyhow` in `main`** (typed matchable
  errors where logic lives, ergonomic context at the CLI edge).
- Crate layout: **`lib.rs` + thin `main.rs`** (modules `model`, `store`, `engine`, `io`;
  `tests/` calls the library directly instead of spawning the binary).

### Q7 — Duplicate withdrawal ids (discovered while consolidating)

We only store deposits, so a withdrawal reusing another withdrawal's id is undetectable
without extra tracking. Leonardo suggested a Bloom filter instead of a HashSet.

Claude's answer: sharp instinct, wrong failure semantics here. A Bloom filter has false
positives — a fresh, legitimate withdrawal can hash-collide into "maybe seen" and get
rejected, corrupting correct output on valid input. Bloom filters earn their keep as
gatekeepers in front of an authoritative store (a "maybe" triggers an exact lookup);
here there is no store behind it — the filter would BE the only record, so its error
rate converts directly into wrongly rejected money movements. That's worse than the
defended-against case (a spec-guaranteed-impossible duplicate, which would anyway be
balance-checked and can't overdraw). Severity asymmetry decides it. Exact alternatives
if tracking were wanted: `HashSet<TxId>` (~6–8 B/entry) or a Roaring bitmap (exact,
compressed — the no-false-positive version of the Bloom idea). A full u32 bitmap is off
the table (512 MiB).

**Decision: don't track withdrawal ids.** README line documents the reasoning.

---

## Design presented and approved

Full design presented in sections (architecture, data model, engine rules table, store,
error handling, testing, README content). Key semantics locked:

- `total` computed at output time (`available + held`), never stored — cannot drift.
- `DepositState { Posted, Disputed, ChargedBack }`; resolve → back to `Posted`
  (re-disputable); `ChargedBack` is terminal.
- Locked blocks only new deposits/withdrawals; dispute-family ops still process.
- Available may go negative (dispute-after-withdrawal fraud case) — deliberate; held never
  negative.
- State never mutated on any `Err` path; check order fixed as listed in the rules table.
- `TxError` variants: `AccountLocked`, `NonPositiveAmount`, `DuplicateTx`,
  `InsufficientFunds`, `UnknownTx`, `ClientMismatch`, `NotDisputable`, `NotUnderDispute`.
- CSV reader: `trim(Trim::All)` + `flexible(true)`; streaming, never loads the file.
- Memory: O(#clients + #deposits) — the scalability answer.
- Deps: `csv`, `serde`, `rust_decimal` (serde feature), `thiserror`, `anyhow`;
  dev: `rust_decimal_macros`.
- Testing: TDD unit tests per rules-table cell (assert exact `TxError`, state untouched on
  rejection) + 7 integration scenarios (spec example, fraud flow, dispute→resolve→
  re-dispute→chargeback, whitespace input, malformed rows, locked-account behavior, all
  ignore rules end-to-end).

## Spec written

`docs/superpowers/specs/2026-07-31-payments-engine-design.md` — self-sufficient for
implementation (all 14 decisions inline with the rules table, testing plan, and README
requirements including the Postgres-ledger and TCP-streams/per-client-ordering answers
from session 1). Folder is not yet a git repo; `cargo new`/`git init` happens in the
implementation session and the spec goes into the first commit.

## Next steps

1. Leonardo reviews the spec (attention points: check ordering in section 5, decision #6
   rationale, the 7 integration scenarios in section 9).
2. Clear context; implement from the spec with TDD.
