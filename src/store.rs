//! In-memory state behind intention-revealing methods. A dumb container:
//! no business rules live here — those belong to [`crate::engine`].

use crate::model::{Account, ClientId, DepositRecord, TxId};
use std::collections::HashMap;

/// Holds all mutable state: per-client accounts and the retained deposits
/// (the only disputable transactions, so the only ones stored).
#[derive(Default)]
pub struct Store {
    accounts: HashMap<ClientId, Account>,
    deposits: HashMap<TxId, DepositRecord>,
}

impl Store {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the client's account, creating a default (zeroed, unlocked)
    /// one on first access. This is how "ghost clients" — accounts whose
    /// only transaction was rejected — end up in the output.
    pub fn account_or_create(&mut self, id: ClientId) -> &mut Account {
        self.accounts.entry(id).or_default()
    }

    pub fn get_account(&self, id: ClientId) -> Option<&Account> {
        self.accounts.get(&id)
    }

    pub fn get_deposit(&self, id: TxId) -> Option<&DepositRecord> {
        self.deposits.get(&id)
    }

    pub fn get_deposit_mut(&mut self, id: TxId) -> Option<&mut DepositRecord> {
        self.deposits.get_mut(&id)
    }

    pub fn insert_deposit(&mut self, id: TxId, record: DepositRecord) {
        self.deposits.insert(id, record);
    }

    /// True if a deposit with this tx id was already stored — the duplicate
    /// check for incoming deposits.
    pub fn contains_tx(&self, id: TxId) -> bool {
        self.deposits.contains_key(&id)
    }

    /// Iterates all accounts for output. Order is unspecified (HashMap
    /// iteration order).
    pub fn iter_accounts(&self) -> impl Iterator<Item = (&ClientId, &Account)> {
        self.accounts.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DepositState;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn posted_deposit(client: u16, amount: Decimal) -> DepositRecord {
        DepositRecord {
            client,
            amount,
            state: DepositState::Posted,
        }
    }

    #[test]
    fn account_or_create_creates_a_default_account_on_first_access() {
        let mut store = Store::new();

        let account = store.account_or_create(1);

        assert_eq!(account.available, Decimal::ZERO);
        assert_eq!(account.held, Decimal::ZERO);
        assert!(!account.locked);
    }

    #[test]
    fn account_or_create_returns_the_same_account_on_later_access() {
        let mut store = Store::new();

        store.account_or_create(1).available = dec!(5);

        let account = store.account_or_create(1);
        assert_eq!(account.available, dec!(5));
    }

    #[test]
    fn accounts_of_different_clients_are_independent() {
        let mut store = Store::new();

        store.account_or_create(1).available = dec!(5);

        let other = store.account_or_create(2);
        assert_eq!(other.available, Decimal::ZERO);
    }

    #[test]
    fn insert_deposit_and_get_deposit_round_trip() {
        let mut store = Store::new();

        store.insert_deposit(10, posted_deposit(1, dec!(2.5)));

        let record = store.get_deposit(10).expect("deposit 10 should be stored");
        assert_eq!(record.client, 1);
        assert_eq!(record.amount, dec!(2.5));
        assert!(matches!(record.state, DepositState::Posted));
    }

    #[test]
    fn get_deposit_on_unknown_id_returns_none() {
        let store = Store::new();

        assert!(store.get_deposit(999).is_none());
    }

    #[test]
    fn contains_tx_is_false_before_insert_and_true_after() {
        let mut store = Store::new();

        assert!(!store.contains_tx(10));

        store.insert_deposit(10, posted_deposit(1, dec!(2.5)));

        assert!(store.contains_tx(10));
    }
}
