use std::collections::HashMap;

use crate::model::{Account, ClientId, DepositRecord, TxId};


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

    pub fn get_deposit(&self, id: TxId) -> Option<&DepositRecord> {
        self.deposits.get(&id)
    }

    pub fn insert_deposit(&mut self, id: TxId, record: DepositRecord) {
        self.deposits.insert(id, record); 
    }

    pub fn contains_tx(&self, id: TxId) -> bool {
        self.deposits.contains_key(&id)
    }

}