mod transcation;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use csv::{ReaderBuilder, Trim, WriterBuilder};
use serde::{Deserialize, Serialize};

pub use crate::transcation::{Transaction, TransactionType};

use self::HwErrors::*;

const AMOUNT_CAN_NOT_BE_PARSED: &str = "Amount could not be parsed";

#[derive(Debug, thiserror::Error)]
pub enum HwErrors {
    #[error("Cli must have exactly one arg supplied, got insted: {0:?}")]
    IncorrectAmountOfArgs(Vec<String>),

    #[error("Transactions cannot be parsed from CsvRow: {0:?}")]
    TransactionCouldNotBeParsed(String),

    #[error("Amount arithmetic overflow, underflow, or out-of-range conversion")]
    ArithmeticError,

    #[error("Transaction amounts must be nonnegative")]
    NegativeAmount,
}

#[derive(Debug, Deserialize)]
struct CsvInputRow {
    r#type: String,
    client: u16,
    tx: u32,
    amount: Option<String>,
}

#[derive(Debug, Serialize)]
struct CsvOutputRow {
    client: u16,
    available: String,
    held: String,
    total: String,
    locked: bool,
}

fn amount_to_str(amount: i64) -> String {
    let sign = if amount < 0 { "-" } else { "" };
    let amount = amount.unsigned_abs();
    let bp = amount % 10_000;
    let whole = amount / 10_000;
    format!("{sign}{whole}.{bp:04}")
}

pub fn get_transactions<P: AsRef<Path>>(path: P) -> anyhow::Result<Vec<Transaction>> {
    let mut reader = ReaderBuilder::new().trim(Trim::All).from_path(path)?;
    let records: Vec<CsvInputRow> = reader
        .deserialize::<CsvInputRow>()
        .collect::<Result<_, _>>()?;
    let transactions: Vec<Transaction> = records
        .into_iter()
        .map(|r| Transaction::try_from(r))
        .collect::<Result<_, _>>()?;
    Ok(transactions)
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
    clients: HashMap<u16, Client>,
    processed_transactions: HashMap<u32, Transaction>,
    disputed_transactions: HashMap<u32, Dispute>,
}

impl PaymentProcessor {
    pub fn client(&self, client_id: u16) -> Option<&Client> {
        self.clients.get(&client_id)
    }

    fn process_transaction(&mut self, transaction: Transaction) -> Result<(), HwErrors> {
        let client_id = transaction.client();
        let tx = transaction.tx();
        let client = self.clients.entry(client_id).or_insert(Client::default());
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

    pub fn process_all_transactions(
        &mut self,
        transactions: Vec<Transaction>,
    ) -> Result<(), HwErrors> {
        for trx in transactions {
            self.process_transaction(trx)?;
        }
        Ok(())
    }

    pub fn report(&self) -> anyhow::Result<()> {
        // This will surely break on non unix things
        let stdout = PathBuf::from("/dev/stdout");
        let mut writer = WriterBuilder::new().from_path(stdout)?;
        for (id, client) in &self.clients {
            let total = client
                .available
                .checked_add(client.held)
                .ok_or(ArithmeticError)?;
            let output = CsvOutputRow {
                client: *id,
                available: amount_to_str(client.available),
                held: amount_to_str(client.held),
                total: amount_to_str(total),
                locked: client.locked,
            };
            writer.serialize(output)?;
        }
        writer.flush()?;
        Ok(())
    }
}
