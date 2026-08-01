use std::collections::HashMap;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use crate::model::{Account, ClientId, DepositRecord, TxId, DepositState};


#[derive(Default)]
pub struct Store {
    accounts: HashMap<ClientId,Account>,
    deposits: HashMap<TxId, DepositRecord>
}

impl Store {
    pub fn new() -> Self {
        Self::default()
    }

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


    pub fn contains_tx(&self, id: TxId) -> bool {
        self.deposits.contains_key(&id)
    }

    pub fn iter_accounts(&self) -> impl Iterator<Item = (&ClientId, &Account)> {
        self.accounts.iter()
    }

}



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
