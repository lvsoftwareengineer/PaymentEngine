use rust_decimal::Decimal;
use thiserror::Error;
pub type ClientId = u16;
pub type TxId = u32;

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

#[derive(Default)]
pub struct Account {
    pub available: Decimal,
    pub held: Decimal,
    pub locked: bool,
}

impl Account {
    pub fn total(&self) -> Decimal {
        self.available + self.held
    }
}

#[derive(PartialEq)]
pub enum DepositState {
    Posted,
    Disputed,
    ChargedBack,
}

pub struct DepositRecord {
    pub client: ClientId,
    pub amount: Decimal,
    pub state: DepositState,
}

#[derive(Debug, Error, PartialEq)]
pub enum TxError {
    #[error("Account {client} is locked")]
    AccountLocked { client: ClientId },

    #[error("Amount {amount} not positive")]
    NonPositiveAmount { amount: Decimal },

    #[error("Transaction {tx} already exists")]
    DuplicateTx { tx: TxId },

    #[error("Insufficient funds: available {available}, requested {requested}")]
    InsufficientFunds {
        available: Decimal,
        requested: Decimal,
    },

    #[error("Transaction {tx} not found")]
    UnknownTx { tx: TxId },

    #[error("Transaction {tx} belongs to client {owner}, not client {claimed}")]
    ClientMismatch {
        tx: TxId,
        owner: ClientId,
        claimed: ClientId,
    },

    #[error("Transaction {tx} is not disputable")]
    NotDisputable { tx: TxId },

    #[error("Transaction {tx} is not under dispute")]
    NotUnderDispute { tx: TxId },

    #[error("Transaction {tx} would overflow the account balance")]
    BalanceOverflow { tx: TxId },
}
