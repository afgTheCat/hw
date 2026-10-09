mod payment_processor;
mod transcation;

use std::path::Path;

use csv::{ReaderBuilder, Trim};
use serde::{Deserialize, Serialize};

pub use crate::transcation::{Transaction, TransactionType};
pub use payment_processor::PaymentProcessor;

#[derive(Debug, thiserror::Error)]
pub enum HwErrors {
    #[error("Cli must have exactly one arg supplied, got insted: {0:?}")]
    IncorrectAmountOfArgs(Vec<String>),

    #[error("Transactions cannot be parsed from CsvRow: {0:?}")]
    TransactionCouldNotBeParsed(String),

    #[error("Amount arithmetic overflow, underflow, or out-of-range conversion")]
    ArithmeticError,

    #[error("Invalid amount {amount:?}: {reason}")]
    InvalidAmount { amount: String, reason: String },
}

impl HwErrors {
    pub fn invalid_amount(amount: impl ToString, reason: impl Into<String>) -> Self {
        Self::InvalidAmount {
            amount: amount.to_string(),
            reason: reason.into(),
        }
    }
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
