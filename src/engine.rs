use rust_decimal::Decimal;
use crate::model::*;
use crate::store::Store;

#[derive(Default)]
pub struct PaymentEngine{
    store: Store
}

impl PaymentEngine{
    pub fn new() -> Self{
        Self::default()
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn process(&mut self, tx: Transaction) -> Result<(), TxError> {
        match tx {
            Transaction::Deposit { client, tx, amount } => self.deposit(client, tx, amount),
            Transaction::Withdraw { client, tx, amount } => self.withdraw(client, tx, amount),
            Transaction::Dispute { client, tx } => self.dispute(client, tx),
            Transaction::Resolve { client, tx } => self.resolve(client, tx),
            Transaction::Chargeback { client, tx } => self.chargeback(client, tx),
        }
    }
    
    fn deposit(&mut self, client: ClientId, tx: TxId, amount: Decimal) -> Result<(), TxError> {
        let account = self.store.account_or_create(client);
    
        if account.locked {
            return Err(TxError::AccountLocked { client });
        }
        if amount <= Decimal::ZERO {
            return Err(TxError::NonPositiveAmount { amount });
        }
        if self.store.contains_tx(tx) {
            return Err(TxError::DuplicateTx { tx });
        }
    
        self.store.account_or_create(client).available += amount;
        self.store.insert_deposit(tx, DepositRecord { client, amount, state: DepositState::Posted });
        Ok(())
    }

    fn withdraw(&mut self, client: ClientId, _tx: TxId, amount: Decimal) -> Result<(), TxError> {
        let account = self.store.account_or_create(client);

        if account.locked {
            return Err(TxError::AccountLocked { client });
        }
        if amount <= Decimal::ZERO {
            return Err(TxError::NonPositiveAmount { amount });
        }

        if account.available < amount {
            return Err(TxError::InsufficientFunds { available: account.available, requested: amount });
        }
        account.available -= amount;
        Ok(())
    }

    fn dispute(&mut self, client: ClientId, tx: TxId) -> Result<(), TxError> {
        let record = self.store.get_deposit_mut(tx).ok_or(TxError::UnknownTx { tx })?;
    
        if record.client != client {
            return Err(TxError::ClientMismatch { tx, owner: record.client, claimed: client });
        }
        if record.state != DepositState::Posted {
            return Err(TxError::NotDisputable { tx });
        }
    
        record.state = DepositState::Disputed;
        let amount = record.amount;
    
        let account = self.store.account_or_create(client);
        account.available -= amount;
        account.held += amount;
        Ok(())
    }
    
    fn resolve(&mut self, client: ClientId, tx: TxId) -> Result<(), TxError> {
        let record = self.store.get_deposit_mut(tx).ok_or(TxError::UnknownTx { tx })?;
    
        if record.client != client {
            return Err(TxError::ClientMismatch { tx, owner: record.client, claimed: client });
        }
        if record.state != DepositState::Disputed {
            return Err(TxError::NotUnderDispute { tx });
        }
    
        record.state = DepositState::Posted;
        let amount = record.amount;
    
        let account = self.store.account_or_create(client);
        account.held -= amount;
        account.available += amount;
        Ok(())
    }
    
    fn chargeback(&mut self, client: ClientId, tx: TxId) -> Result<(), TxError> {
        let record = self.store.get_deposit_mut(tx).ok_or(TxError::UnknownTx { tx })?;
    
        if record.client != client {
            return Err(TxError::ClientMismatch { tx, owner: record.client, claimed: client });
        }
        if record.state != DepositState::Disputed {
            return Err(TxError::NotUnderDispute { tx });
        }
    
        record.state = DepositState::ChargedBack;
        let amount = record.amount;
    
        let account = self.store.account_or_create(client);
        account.held -= amount;
        account.locked = true;
        Ok(())
    }

}




#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn deposit(client: ClientId, tx: TxId, amount: Decimal) -> Transaction {
        Transaction::Deposit { client, tx, amount }
    }

    fn withdraw(client: ClientId, tx: TxId, amount: Decimal) -> Transaction {
        Transaction::Withdraw { client, tx, amount }
    }

    // --- Deposit ---

    #[test]
    fn deposit_increases_available_and_stores_posted_record() {
        let mut engine = PaymentEngine::new();

        let result = engine.process(deposit(1, 1, dec!(2.5)));

        assert_eq!(result, Ok(()));

        let account = engine.store.account_or_create(1);
        assert_eq!(account.available, dec!(2.5));
        assert_eq!(account.held, Decimal::ZERO);
        assert!(!account.locked);

        let record = engine.store.get_deposit(1).expect("deposit 1 should be stored");
        assert_eq!(record.client, 1);
        assert_eq!(record.amount, dec!(2.5));
        assert!(matches!(record.state, DepositState::Posted));
    }

    #[test]
    fn deposit_on_locked_account_is_rejected_and_state_untouched() {
        let mut engine = PaymentEngine::new();
        engine.store.account_or_create(1).locked = true;

        let result = engine.process(deposit(1, 1, dec!(2.5)));

        assert_eq!(result, Err(TxError::AccountLocked { client: 1 }));
        assert_eq!(engine.store.account_or_create(1).available, Decimal::ZERO);
        assert!(!engine.store.contains_tx(1));
    }

    #[test]
    fn deposit_of_zero_amount_is_rejected() {
        let mut engine = PaymentEngine::new();

        let result = engine.process(deposit(1, 1, dec!(0)));

        assert_eq!(result, Err(TxError::NonPositiveAmount { amount: dec!(0) }));
        assert!(!engine.store.contains_tx(1));
    }

    #[test]
    fn deposit_of_negative_amount_is_rejected() {
        let mut engine = PaymentEngine::new();

        let result = engine.process(deposit(1, 1, dec!(-3)));

        assert_eq!(result, Err(TxError::NonPositiveAmount { amount: dec!(-3) }));
        assert!(!engine.store.contains_tx(1));
    }

    #[test]
    fn duplicate_deposit_tx_id_is_rejected_and_first_wins() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(2.5))).unwrap();

        let result = engine.process(deposit(1, 1, dec!(9)));

        assert_eq!(result, Err(TxError::DuplicateTx { tx: 1 }));
        assert_eq!(engine.store.account_or_create(1).available, dec!(2.5));
        assert_eq!(engine.store.get_deposit(1).unwrap().amount, dec!(2.5));
    }

    #[test]
    fn rejected_deposit_still_creates_the_account() {
        let mut engine = PaymentEngine::new();

        let _ = engine.process(deposit(7, 1, dec!(0)));

        assert!(engine.store.get_account(7).is_some());
    }

    #[test]
    fn locked_is_checked_before_amount_validation() {
        let mut engine = PaymentEngine::new();
        engine.store.account_or_create(1).locked = true;

        let result = engine.process(deposit(1, 1, dec!(-1)));

        assert_eq!(result, Err(TxError::AccountLocked { client: 1 }));
    }

    // --- Withdrawal ---

    #[test]
    fn withdrawal_decreases_available_and_is_not_stored() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();

        let result = engine.process(withdraw(1, 2, dec!(1.5)));

        assert_eq!(result, Ok(()));
        assert_eq!(engine.store.account_or_create(1).available, dec!(3.5));
        assert!(!engine.store.contains_tx(2));
    }

    #[test]
    fn withdrawal_on_locked_account_is_rejected_and_state_untouched() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();
        engine.store.account_or_create(1).locked = true;

        let result = engine.process(withdraw(1, 2, dec!(1.5)));

        assert_eq!(result, Err(TxError::AccountLocked { client: 1 }));
        assert_eq!(engine.store.account_or_create(1).available, dec!(5));
    }

    #[test]
    fn withdrawal_of_zero_amount_is_rejected() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();

        let result = engine.process(withdraw(1, 2, dec!(0)));

        assert_eq!(result, Err(TxError::NonPositiveAmount { amount: dec!(0) }));
        assert_eq!(engine.store.account_or_create(1).available, dec!(5));
    }

    #[test]
    fn withdrawal_of_negative_amount_is_rejected() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();

        let result = engine.process(withdraw(1, 2, dec!(-1)));

        assert_eq!(result, Err(TxError::NonPositiveAmount { amount: dec!(-1) }));
        assert_eq!(engine.store.account_or_create(1).available, dec!(5));
    }

    #[test]
    fn withdrawal_beyond_available_is_rejected_and_balance_unchanged() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(1))).unwrap();

        let result = engine.process(withdraw(1, 2, dec!(2)));

        assert_eq!(
            result,
            Err(TxError::InsufficientFunds { available: dec!(1), requested: dec!(2) })
        );
        assert_eq!(engine.store.account_or_create(1).available, dec!(1));
    }

    #[test]
    fn withdrawal_of_exactly_available_succeeds() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();

        let result = engine.process(withdraw(1, 2, dec!(5)));

        assert_eq!(result, Ok(()));
        assert_eq!(engine.store.account_or_create(1).available, Decimal::ZERO);
    }

    #[test]
    fn rejected_withdrawal_still_creates_the_account() {
        let mut engine = PaymentEngine::new();

        let result = engine.process(withdraw(9, 1, dec!(1)));

        assert_eq!(
            result,
            Err(TxError::InsufficientFunds { available: dec!(0), requested: dec!(1) })
        );
        assert!(engine.store.get_account(9).is_some());
    }
    // --- Dispute ---

    fn dispute(client: ClientId, tx: TxId) -> Transaction {
        Transaction::Dispute { client, tx }
    }

    #[test]
    fn dispute_holds_the_deposited_amount() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();

        let result = engine.process(dispute(1, 1));

        assert_eq!(result, Ok(()));

        let account = engine.store.account_or_create(1);
        assert_eq!(account.available, Decimal::ZERO);
        assert_eq!(account.held, dec!(5));

        let record = engine.store.get_deposit(1).unwrap();
        assert!(matches!(record.state, DepositState::Disputed));
    }

    #[test]
    fn dispute_on_unknown_tx_is_rejected_and_creates_no_account() {
        let mut engine = PaymentEngine::new();

        let result = engine.process(dispute(1, 99));

        assert_eq!(result, Err(TxError::UnknownTx { tx: 99 }));
        assert!(engine.store.get_account(1).is_none());
    }

    #[test]
    fn dispute_on_a_withdrawal_id_is_rejected_as_unknown() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();
        engine.process(withdraw(1, 2, dec!(1))).unwrap();

        let result = engine.process(dispute(1, 2));

        assert_eq!(result, Err(TxError::UnknownTx { tx: 2 }));
        assert_eq!(engine.store.account_or_create(1).held, Decimal::ZERO);
    }

    #[test]
    fn dispute_by_the_wrong_client_is_rejected_and_state_untouched() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();

        let result = engine.process(dispute(2, 1));

        assert_eq!(
            result,
            Err(TxError::ClientMismatch { tx: 1, owner: 1, claimed: 2 })
        );

        let account = engine.store.account_or_create(1);
        assert_eq!(account.available, dec!(5));
        assert_eq!(account.held, Decimal::ZERO);
        assert!(matches!(
            engine.store.get_deposit(1).unwrap().state,
            DepositState::Posted
        ));
    }

    #[test]
    fn double_dispute_is_rejected_and_state_untouched() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();
        engine.process(dispute(1, 1)).unwrap();

        let result = engine.process(dispute(1, 1));

        assert_eq!(result, Err(TxError::NotDisputable { tx: 1 }));

        let account = engine.store.account_or_create(1);
        assert_eq!(account.available, Decimal::ZERO);
        assert_eq!(account.held, dec!(5));
        assert!(matches!(
            engine.store.get_deposit(1).unwrap().state,
            DepositState::Disputed
        ));
    }

    #[test]
    fn dispute_on_a_charged_back_deposit_is_rejected() {
        let mut engine = PaymentEngine::new();
        engine.store.insert_deposit(
            1,
            DepositRecord { client: 1, amount: dec!(5), state: DepositState::ChargedBack },
        );

        let result = engine.process(dispute(1, 1));

        assert_eq!(result, Err(TxError::NotDisputable { tx: 1 }));
        assert!(matches!(
            engine.store.get_deposit(1).unwrap().state,
            DepositState::ChargedBack
        ));
    }

    #[test]
    fn dispute_after_withdrawal_drives_available_negative() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();
        engine.process(withdraw(1, 2, dec!(5))).unwrap();

        let result = engine.process(dispute(1, 1));

        assert_eq!(result, Ok(()));

        let account = engine.store.account_or_create(1);
        assert_eq!(account.available, dec!(-5));
        assert_eq!(account.held, dec!(5));
        assert_eq!(account.total(), Decimal::ZERO);
    }

    #[test]
    fn dispute_on_a_locked_account_still_processes() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();
        engine.store.account_or_create(1).locked = true;

        let result = engine.process(dispute(1, 1));

        assert_eq!(result, Ok(()));
        assert_eq!(engine.store.account_or_create(1).held, dec!(5));
    }

    #[test]
    fn unknown_tx_is_checked_before_client_mismatch() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();

        let result = engine.process(dispute(2, 99));

        assert_eq!(result, Err(TxError::UnknownTx { tx: 99 }));
    }
    // --- Resolve ---

    fn resolve(client: ClientId, tx: TxId) -> Transaction {
        Transaction::Resolve { client, tx }
    }

    #[test]
    fn resolve_releases_the_held_amount_back_to_available() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();
        engine.process(dispute(1, 1)).unwrap();

        let result = engine.process(resolve(1, 1));

        assert_eq!(result, Ok(()));

        let account = engine.store.account_or_create(1);
        assert_eq!(account.available, dec!(5));
        assert_eq!(account.held, Decimal::ZERO);
        assert!(matches!(
            engine.store.get_deposit(1).unwrap().state,
            DepositState::Posted
        ));
    }

    #[test]
    fn resolve_on_unknown_tx_is_rejected() {
        let mut engine = PaymentEngine::new();

        let result = engine.process(resolve(1, 99));

        assert_eq!(result, Err(TxError::UnknownTx { tx: 99 }));
    }

    #[test]
    fn resolve_by_the_wrong_client_is_rejected_and_state_untouched() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();
        engine.process(dispute(1, 1)).unwrap();

        let result = engine.process(resolve(2, 1));

        assert_eq!(
            result,
            Err(TxError::ClientMismatch { tx: 1, owner: 1, claimed: 2 })
        );
        assert_eq!(engine.store.account_or_create(1).held, dec!(5));
        assert!(matches!(
            engine.store.get_deposit(1).unwrap().state,
            DepositState::Disputed
        ));
    }

    #[test]
    fn resolve_without_an_open_dispute_is_rejected() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();

        let result = engine.process(resolve(1, 1));

        assert_eq!(result, Err(TxError::NotUnderDispute { tx: 1 }));

        let account = engine.store.account_or_create(1);
        assert_eq!(account.available, dec!(5));
        assert_eq!(account.held, Decimal::ZERO);
    }

    #[test]
    fn resolve_on_a_charged_back_deposit_is_rejected() {
        let mut engine = PaymentEngine::new();
        engine.store.insert_deposit(
            1,
            DepositRecord { client: 1, amount: dec!(5), state: DepositState::ChargedBack },
        );

        let result = engine.process(resolve(1, 1));

        assert_eq!(result, Err(TxError::NotUnderDispute { tx: 1 }));
        assert!(matches!(
            engine.store.get_deposit(1).unwrap().state,
            DepositState::ChargedBack
        ));
    }

    #[test]
    fn re_dispute_after_resolve_is_allowed() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();
        engine.process(dispute(1, 1)).unwrap();
        engine.process(resolve(1, 1)).unwrap();

        let result = engine.process(dispute(1, 1));

        assert_eq!(result, Ok(()));
        assert_eq!(engine.store.account_or_create(1).held, dec!(5));
        assert!(matches!(
            engine.store.get_deposit(1).unwrap().state,
            DepositState::Disputed
        ));
    }

    #[test]
    fn resolve_on_a_locked_account_still_processes() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();
        engine.process(dispute(1, 1)).unwrap();
        engine.store.account_or_create(1).locked = true;

        let result = engine.process(resolve(1, 1));

        assert_eq!(result, Ok(()));

        let account = engine.store.account_or_create(1);
        assert_eq!(account.available, dec!(5));
        assert_eq!(account.held, Decimal::ZERO);
    }

    // --- Chargeback ---

    fn chargeback(client: ClientId, tx: TxId) -> Transaction {
        Transaction::Chargeback { client, tx }
    }

    #[test]
    fn chargeback_removes_held_funds_and_locks_the_account() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();
        engine.process(dispute(1, 1)).unwrap();

        let result = engine.process(chargeback(1, 1));

        assert_eq!(result, Ok(()));

        let account = engine.store.account_or_create(1);
        assert_eq!(account.available, Decimal::ZERO);
        assert_eq!(account.held, Decimal::ZERO);
        assert_eq!(account.total(), Decimal::ZERO);
        assert!(account.locked);
        assert!(matches!(
            engine.store.get_deposit(1).unwrap().state,
            DepositState::ChargedBack
        ));
    }

    #[test]
    fn chargeback_on_unknown_tx_is_rejected() {
        let mut engine = PaymentEngine::new();

        let result = engine.process(chargeback(1, 99));

        assert_eq!(result, Err(TxError::UnknownTx { tx: 99 }));
    }

    #[test]
    fn chargeback_by_the_wrong_client_is_rejected_and_state_untouched() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();
        engine.process(dispute(1, 1)).unwrap();

        let result = engine.process(chargeback(2, 1));

        assert_eq!(
            result,
            Err(TxError::ClientMismatch { tx: 1, owner: 1, claimed: 2 })
        );

        let account = engine.store.account_or_create(1);
        assert_eq!(account.held, dec!(5));
        assert!(!account.locked);
        assert!(matches!(
            engine.store.get_deposit(1).unwrap().state,
            DepositState::Disputed
        ));
    }

    #[test]
    fn chargeback_without_an_open_dispute_is_rejected() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();

        let result = engine.process(chargeback(1, 1));

        assert_eq!(result, Err(TxError::NotUnderDispute { tx: 1 }));
        assert!(!engine.store.account_or_create(1).locked);
    }

    #[test]
    fn chargeback_twice_is_rejected() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();
        engine.process(dispute(1, 1)).unwrap();
        engine.process(chargeback(1, 1)).unwrap();

        let result = engine.process(chargeback(1, 1));

        assert_eq!(result, Err(TxError::NotUnderDispute { tx: 1 }));
        assert_eq!(engine.store.account_or_create(1).held, Decimal::ZERO);
    }

    #[test]
    fn deposits_and_withdrawals_are_blocked_after_chargeback() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();
        engine.process(dispute(1, 1)).unwrap();
        engine.process(chargeback(1, 1)).unwrap();

        assert_eq!(
            engine.process(deposit(1, 2, dec!(1))),
            Err(TxError::AccountLocked { client: 1 })
        );
        assert_eq!(
            engine.process(withdraw(1, 3, dec!(1))),
            Err(TxError::AccountLocked { client: 1 })
        );
    }

    #[test]
    fn dispute_on_another_posted_deposit_still_processes_after_chargeback() {
        let mut engine = PaymentEngine::new();
        engine.process(deposit(1, 1, dec!(5))).unwrap();
        engine.process(deposit(1, 2, dec!(3))).unwrap();
        engine.process(dispute(1, 1)).unwrap();
        engine.process(chargeback(1, 1)).unwrap();

        let result = engine.process(dispute(1, 2));

        assert_eq!(result, Ok(()));
        assert_eq!(engine.store.account_or_create(1).held, dec!(3));
        assert!(matches!(
            engine.store.get_deposit(2).unwrap().state,
            DepositState::Disputed
        ));
    }
}
