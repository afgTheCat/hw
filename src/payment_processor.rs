use csv::WriterBuilder;
use rustc_hash::FxHashMap;

use crate::PaymentError::{self, ArithmeticError};
use crate::transaction::DisputeStatus;
use crate::{AccountCsvRow, Transaction, TransactionKind};

fn format_amount(amount: i128) -> String {
    let sign = if amount < 0 { "-" } else { "" };
    let amount = amount.unsigned_abs();
    let bp = amount % 10_000;
    let whole = amount / 10_000;
    format!("{sign}{whole}.{bp:04}")
}

#[derive(Default)]
pub struct Account {
    pub available: i64,
    pub held: i64,
    pub locked: bool,
}

impl Account {
    fn deposit(&mut self, amount: i64) -> Result<bool, PaymentError> {
        if self.locked {
            Ok(false)
        } else {
            self.available = self.available.checked_add(amount).ok_or(ArithmeticError)?;
            Ok(true)
        }
    }

    fn withdraw(&mut self, amount: i64) -> Result<bool, PaymentError> {
        if !self.locked && self.available >= amount {
            self.available = self.available.checked_sub(amount).ok_or(ArithmeticError)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn hold_funds(&mut self, amount: i64) -> Result<(), PaymentError> {
        let available = self.available.checked_sub(amount).ok_or(ArithmeticError)?;
        let held = self.held.checked_add(amount).ok_or(ArithmeticError)?;
        self.available = available;
        self.held = held;
        Ok(())
    }

    fn release_held_funds(&mut self, amount: i64) -> Result<(), PaymentError> {
        let held = self.held.checked_sub(amount).ok_or(ArithmeticError)?;
        let available = self.available.checked_add(amount).ok_or(ArithmeticError)?;
        self.held = held;
        self.available = available;
        Ok(())
    }

    fn apply_chargeback(&mut self, amount: i64) -> Result<(), PaymentError> {
        self.held = self.held.checked_sub(amount).ok_or(ArithmeticError)?;
        self.locked = true;
        Ok(())
    }
}

#[derive(Default)]
pub struct PaymentProcessor {
    accounts: FxHashMap<u16, Account>,
    processed_transactions: FxHashMap<u32, Transaction>,
}

impl PaymentProcessor {
    #[must_use]
    pub fn account(&self, client_id: u16) -> Option<&Account> {
        self.accounts.get(&client_id)
    }

    /// # Errors
    /// Returns `PaymentError::ArithmeticError` if a balance update overflows.
    /// The failed transaction's balance updates are not applied.
    pub fn process_transaction(&mut self, transaction: Transaction) -> Result<(), PaymentError> {
        let client_id = transaction.client_id();
        let tx = transaction.transaction_id();
        let account = self.accounts.entry(client_id).or_default();
        match transaction.kind() {
            TransactionKind::Deposit { amount } => {
                if account.deposit(*amount)? {
                    self.processed_transactions.insert(tx, transaction);
                }
            }
            TransactionKind::Withdrawal { amount } => {
                if account.withdraw(*amount)? {
                    self.processed_transactions.insert(tx, transaction);
                }
            }
            TransactionKind::Dispute => {
                let Some(transaction) = self.processed_transactions.get_mut(&tx) else {
                    return Ok(());
                };
                let Some(amount) = transaction.disputable_amount(client_id) else {
                    return Ok(());
                };
                account.hold_funds(amount)?;
                transaction.set_status(DisputeStatus::Disputed);
            }
            TransactionKind::Resolve => {
                let Some(transaction) = self.processed_transactions.get_mut(&tx) else {
                    return Ok(());
                };
                let Some(amount) = transaction.disputed_amount(client_id) else {
                    return Ok(());
                };
                account.release_held_funds(amount)?;
                transaction.set_status(DisputeStatus::Undisputed);
            }
            TransactionKind::Chargeback => {
                let Some(transaction) = self.processed_transactions.get_mut(&tx) else {
                    return Ok(());
                };
                let Some(amount) = transaction.disputed_amount(client_id) else {
                    return Ok(());
                };
                account.apply_chargeback(amount)?;
                transaction.set_status(DisputeStatus::ChargedBack);
            }
        }
        Ok(())
    }

    /// # Errors
    /// Returns the first processing error; earlier transactions remain applied.
    pub fn process_transactions(
        &mut self,
        transactions: Vec<Transaction>,
    ) -> Result<(), PaymentError> {
        for trx in transactions {
            self.process_transaction(trx)?;
        }
        Ok(())
    }

    /// # Errors
    /// Returns an error if CSV serialization or writing to stdout fails.
    pub fn write_accounts_csv(&self) -> anyhow::Result<()> {
        let stdout = std::io::stdout();
        let mut writer = WriterBuilder::new()
            .has_headers(false)
            .from_writer(stdout.lock());
        writer.write_record(["client", "available", "held", "total", "locked"])?;
        for (id, account) in &self.accounts {
            let available = i128::from(account.available);
            let held = i128::from(account.held);
            let total = available + held;
            let output = AccountCsvRow {
                client: *id,
                available: format_amount(available),
                held: format_amount(held),
                total: format_amount(total),
                locked: account.locked,
            };
            writer.serialize(output)?;
        }
        writer.flush()?;
        Ok(())
    }
}
