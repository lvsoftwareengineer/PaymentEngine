//! Domain types shared by the whole crate: transactions, accounts, deposit
//! records, and the typed rejection errors. Pure data — no behavior beyond
//! [`Account::total`], and no I/O.

use rust_decimal::Decimal;
use thiserror::Error;

/// Client identifier. `u16` per the spec — bounds the account universe.
pub type ClientId = u16;

/// Transaction identifier. `u32` per the spec, unique among
/// deposits/withdrawals only; dispute-family rows reference an existing id.
pub type TxId = u32;

/// A validated transaction, ready for [`crate::engine::PaymentEngine::process`].
///
/// Deposits and withdrawals carry their own `tx` id and amount. The
/// dispute-family variants (`Dispute`, `Resolve`, `Chargeback`) carry no
/// amount: their `tx` references the deposit they act on, and the amount is
/// taken from the stored [`DepositRecord`].
#[derive(Debug, PartialEq)]
pub enum Transaction {
    Deposit {
        client: ClientId,
        tx: TxId,
        amount: Decimal,
    },
    Withdraw {
        client: ClientId,
        tx: TxId,
        amount: Decimal,
    },
    Dispute {
        client: ClientId,
        tx: TxId,
    },
    Resolve {
        client: ClientId,
        tx: TxId,
    },
    Chargeback {
        client: ClientId,
        tx: TxId,
    },
}

/// A client's account balances and frozen status.
///
/// There is deliberately no `total` field — see [`Account::total`].
#[derive(Default)]
pub struct Account {
    /// Funds the client can withdraw. May go negative: dispute a deposit
    /// that was already spent and the shortfall is the client's debt.
    pub available: Decimal,
    /// Funds frozen by open disputes. Never negative.
    pub held: Decimal,
    /// Set (permanently) by a chargeback. Blocks new deposits and
    /// withdrawals only — disputes, resolves and chargebacks on
    /// pre-existing deposits still process.
    pub locked: bool,
}

impl Account {
    /// Total funds (`available + held`), computed on demand and never
    /// stored, so it cannot drift out of sync.
    pub fn total(&self) -> Decimal {
        self.available + self.held
    }
}

/// Dispute lifecycle of a deposit.
///
/// Transitions: `Posted ⇄ Disputed → ChargedBack`. A resolved dispute may
/// be disputed again; `ChargedBack` is terminal.
#[derive(PartialEq)]
pub enum DepositState {
    Posted,
    Disputed,
    ChargedBack,
}

/// A retained deposit, kept so later dispute-family transactions can find
/// its owner and amount. Only deposits are stored — they are the only
/// disputable entity, which is what keeps memory `O(#clients + #deposits)`.
pub struct DepositRecord {
    pub client: ClientId,
    pub amount: Decimal,
    pub state: DepositState,
}

/// Why the engine rejected a transaction.
///
/// Every rejection is typed — the caller decides what to do with it (the
/// CLI logs to stderr and moves on). The engine guarantees state is
/// untouched whenever one of these is returned.
#[derive(Debug, Error, PartialEq)]
pub enum TxError {
    /// Deposit or withdrawal on an account frozen by a chargeback.
    #[error("Account {client} is locked")]
    AccountLocked { client: ClientId },

    /// Deposit or withdrawal with a zero or negative amount.
    #[error("Amount {amount} not positive")]
    NonPositiveAmount { amount: Decimal },

    /// Deposit reusing an existing tx id — the first deposit wins.
    #[error("Transaction {tx} already exists")]
    DuplicateTx { tx: TxId },

    /// Withdrawal larger than the available balance.
    #[error("Insufficient funds: available {available}, requested {requested}")]
    InsufficientFunds {
        available: Decimal,
        requested: Decimal,
    },

    /// Dispute-family transaction referencing a tx id that was never
    /// stored (never deposited, or a withdrawal — those are not retained).
    #[error("Transaction {tx} not found")]
    UnknownTx { tx: TxId },

    /// Dispute-family transaction whose client is not the deposit's owner.
    #[error("Transaction {tx} belongs to client {owner}, not client {claimed}")]
    ClientMismatch {
        tx: TxId,
        owner: ClientId,
        claimed: ClientId,
    },

    /// Dispute on a deposit that is not `Posted` (already disputed, or
    /// charged back — terminal).
    #[error("Transaction {tx} is not disputable")]
    NotDisputable { tx: TxId },

    /// Resolve or chargeback on a deposit with no open dispute.
    #[error("Transaction {tx} is not under dispute")]
    NotUnderDispute { tx: TxId },

    #[error("Transaction {tx} would overflow the account balance")]
    BalanceOverflow { tx: TxId },
}
