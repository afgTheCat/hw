use csv::WriterBuilder;
use rustc_hash::FxHashMap;

use crate::PaymentError::{self, ArithmeticError};
use crate::{CsvOutputRow, Transaction, TransactionType};

fn amount_to_str(amount: i128) -> String {
    let sign = if amount < 0 { "-" } else { "" };
    let amount = amount.unsigned_abs();
    let bp = amount % 10_000;
    let whole = amount / 10_000;
    format!("{sign}{whole}.{bp:04}")
}

#[derive(Default)]
pub struct Client {
    pub available: i64,
    pub held: i64,
    pub locked: bool,
}

enum DisputeStatus {
    Pending,
    Resolved,
    Chargedback,
}

struct Dispute {
    transaction: Transaction,
    status: DisputeStatus,
}

impl Dispute {
    fn new(transaction: Transaction) -> Self {
        Self {
            transaction,
            status: DisputeStatus::Pending,
        }
    }
}

#[derive(Default)]
pub struct PaymentProcessor {
    clients: FxHashMap<u16, Client>,
    processed_transactions: FxHashMap<u32, Transaction>,
    disputed_transactions: FxHashMap<u32, Dispute>,
}

impl PaymentProcessor {
    #[must_use]
    pub fn client(&self, client_id: u16) -> Option<&Client> {
        self.clients.get(&client_id)
    }

    /// # Errors
    /// Returns `PaymentError::ArithmeticError` if a balance update overflows.
    /// The failed transaction's balance updates are not applied.
    ///
    /// # Panics
    /// Panics if an internal dispute references a non-deposit transaction.
    /// Disputes are only created for deposits, so valid state prevents this.
    #[expect(
        clippy::too_many_lines,
        reason = "Keep the five transaction cases together in a single dispatch"
    )]
    pub fn process_transaction(&mut self, transaction: Transaction) -> Result<(), PaymentError> {
        let client_id = transaction.client();
        let tx = transaction.tx();
        let client = self.clients.entry(client_id).or_default();
        match transaction.kind() {
            TransactionType::Deposit { amount } => {
                if !client.locked {
                    client.available = client
                        .available
                        .checked_add(*amount)
                        .ok_or(ArithmeticError)?;
                    self.processed_transactions.insert(tx, transaction);
                }
            }
            TransactionType::Withdrawal { amount } => {
                if client.locked {
                    return Ok(());
                }
                if client.available >= *amount {
                    client.available = client
                        .available
                        .checked_sub(*amount)
                        .ok_or(ArithmeticError)?;
                    self.processed_transactions.insert(tx, transaction);
                }
            }
            TransactionType::Dispute => {
                // transaction does not exists
                let Some(disputed_transaction) = self.processed_transactions.get(&tx) else {
                    return Ok(());
                };
                if disputed_transaction.client() != client_id {
                    return Ok(());
                }
                // the transaction in question was not a deposit
                let Some(deposit_amount) = disputed_transaction.deposit_amount() else {
                    return Ok(());
                };
                // transaction is already disputed
                match self.disputed_transactions.get_mut(&tx) {
                    Some(Dispute { status, .. }) if matches!(status, DisputeStatus::Resolved) => {
                        let available = client
                            .available
                            .checked_sub(deposit_amount)
                            .ok_or(ArithmeticError)?;
                        let held = client
                            .held
                            .checked_add(deposit_amount)
                            .ok_or(ArithmeticError)?;
                        client.available = available;
                        client.held = held;
                        *status = DisputeStatus::Pending;
                    }
                    None => {
                        let available = client
                            .available
                            .checked_sub(deposit_amount)
                            .ok_or(ArithmeticError)?;
                        let held = client
                            .held
                            .checked_add(deposit_amount)
                            .ok_or(ArithmeticError)?;
                        client.available = available;
                        client.held = held;
                        let dispute = Dispute::new(disputed_transaction.clone());
                        self.disputed_transactions.insert(tx, dispute);
                    }
                    _ => {}
                }
            }
            TransactionType::Resolve => {
                let Some(Dispute {
                    transaction,
                    status,
                }) = self.disputed_transactions.get_mut(&tx)
                else {
                    return Ok(());
                };
                if let DisputeStatus::Pending = status
                    && transaction.client() == client_id
                {
                    // we now that this is a deposit
                    let amount = transaction.deposit_amount().unwrap();
                    let held = client.held.checked_sub(amount).ok_or(ArithmeticError)?;
                    let available = client
                        .available
                        .checked_add(amount)
                        .ok_or(ArithmeticError)?;
                    client.held = held;
                    client.available = available;
                    *status = DisputeStatus::Resolved;
                }
            }
            TransactionType::Chargeback => {
                let Some(Dispute {
                    transaction,
                    status,
                }) = self.disputed_transactions.get_mut(&tx)
                else {
                    return Ok(());
                };
                if let DisputeStatus::Pending = status
                    && transaction.client() == client_id
                {
                    let amount = transaction.deposit_amount().unwrap();
                    client.held = client.held.checked_sub(amount).ok_or(ArithmeticError)?;
                    client.locked = true;
                    *status = DisputeStatus::Chargedback;
                }
            }
        }
        Ok(())
    }

    /// # Errors
    /// Returns the first processing error; earlier transactions remain applied.
    pub fn process_all_transactions(
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
    pub fn report(&self) -> anyhow::Result<()> {
        let stdout = std::io::stdout();
        let mut writer = WriterBuilder::new()
            .has_headers(false)
            .from_writer(stdout.lock());
        // solution based on codex.
        writer.write_record(["client", "available", "held", "total", "locked"])?;
        for (id, client) in &self.clients {
            let available = i128::from(client.available);
            let held = i128::from(client.held);
            let total = available + held;
            let output = CsvOutputRow {
                client: *id,
                available: amount_to_str(available),
                held: amount_to_str(held),
                total: amount_to_str(total),
                locked: client.locked,
            };
            writer.serialize(output)?;
        }
        writer.flush()?;
        Ok(())
    }
}
