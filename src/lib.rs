mod error;
mod payment_processor;
mod transaction;

use std::path::Path;

use csv::{ReaderBuilder, Trim};
use serde::{Deserialize, Serialize};

pub use crate::transaction::{Transaction, TransactionKind};
pub use error::PaymentError;
pub use payment_processor::PaymentProcessor;

#[derive(Debug, Deserialize, Serialize)]
pub struct TransactionCsvRow {
    r#type: String,
    client: u16,
    tx: u32,
    amount: Option<String>,
}

#[derive(Debug, Serialize)]
struct AccountCsvRow {
    client: u16,
    available: String,
    held: String,
    total: String,
    locked: bool,
}

/// # Errors
/// Returns an error if reading or parsing the input, processing a transaction,
/// or writing the report fails.
pub fn process_csv_file<P: AsRef<Path>>(path: P) -> anyhow::Result<()> {
    let mut payment_processor = PaymentProcessor::default();
    let mut reader = ReaderBuilder::new().trim(Trim::All).from_path(path)?;
    for record in reader.deserialize::<TransactionCsvRow>() {
        let record = record?;
        let transaction = Transaction::try_from(record)?;
        payment_processor.process_transaction(transaction)?;
    }
    payment_processor.write_accounts_csv()?;
    Ok(())
}
