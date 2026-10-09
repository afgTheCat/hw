use std::{collections::HashMap, env::current_dir, path::PathBuf};

use csv::{ReaderBuilder, Trim, WriterBuilder};
use serde::{Deserialize, Serialize};

use crate::HwErrors::*;

const AMOUNT_CAN_NOT_BE_PARSED: &str = "Amount could not be parsed";

#[derive(Debug, thiserror::Error)]
enum HwErrors {
    #[error("Cli must have exactly one arg supplied, got insted: {0:?}")]
    IncorrectAmountOfArgs(Vec<String>),

    #[error("Transactions cannot be parsed from CsvRow: {0:?}")]
    TransactionCouldNotBeParsed(String),
}

#[derive(Debug, Deserialize)]
struct CsvInputRow {
    r#type: String,
    client: u16,
    tx: u32,
    amount: Option<String>,
}

// Amount is stored in bps in order to avoid floating point shenanigans
#[derive(Debug, Clone)]
enum Transaction {
    Deposit { client: u16, tx: u32, amount: u64 },
    Withdrawal { client: u16, tx: u32, amount: u64 },
    Dispute { client: u16, tx: u32 },
    Resolve { client: u16, tx: u32 },
    Chargeback { client: u16, tx: u32 },
}

impl Transaction {
    fn deposit_amount(&self) -> Option<u64> {
        match self {
            Self::Deposit { amount, .. } => Some(*amount),
            _ => None,
        }
    }

    fn client(&self) -> u16 {
        match self {
            Transaction::Deposit { client, .. } => *client,
            Transaction::Withdrawal { client, .. } => *client,
            Transaction::Dispute { client, .. } => *client,
            Transaction::Resolve { client, .. } => *client,
            Transaction::Chargeback { client, .. } => *client,
        }
    }
}

fn parse_amount(amount: &str) -> Result<u64, HwErrors> {
    let Some((amount, bp)) = amount.split_once(".") else {
        let err = "Amount without '.' separator".into();
        return Err(TransactionCouldNotBeParsed(err));
    };
    let bp_decimals = bp.len();
    if bp_decimals > 4 {
        let err = "Amount with higher precision than bp".into();
        return Err(TransactionCouldNotBeParsed(err));
    }
    let amount = u64::from_str_radix(amount, 10)
        .map_err(|_err| TransactionCouldNotBeParsed(AMOUNT_CAN_NOT_BE_PARSED.into()))?;
    let bp = u64::from_str_radix(bp, 10)
        .map_err(|_err| TransactionCouldNotBeParsed(AMOUNT_CAN_NOT_BE_PARSED.into()))?;
    Ok(amount * 10_000 + bp * 10u64.pow(4 - bp_decimals as u32))
}

fn amount_to_str(amount: u64) -> String {
    let bp = amount % 10_000;
    let whole = amount / 10_000;
    format!("{whole}.{bp:04}")
}

impl TryFrom<CsvInputRow> for Transaction {
    type Error = HwErrors;

    fn try_from(value: CsvInputRow) -> Result<Self, Self::Error> {
        let CsvInputRow { client, tx, .. } = value;
        match (value.r#type.as_str(), value.amount) {
            ("deposit", Some(amount)) => {
                let amount = parse_amount(&amount)?;
                Ok(Transaction::Deposit { client, tx, amount })
            }
            ("withdrawal", Some(amount)) => {
                let amount = parse_amount(&amount)?;
                Ok(Transaction::Withdrawal { client, tx, amount })
            }
            ("dispute", None) => Ok(Transaction::Dispute { client, tx }),
            ("resolve", None) => Ok(Transaction::Resolve { client, tx }),
            ("Chargeback", None) => Ok(Transaction::Chargeback { client, tx }),
            (r#type, value) => {
                let error = format!(
                    "Unrecognized transaction type: {} with amount: {value:?}",
                    r#type
                );
                Err(TransactionCouldNotBeParsed(error))
            }
        }
    }
}

#[derive(Default)]
struct Client {
    available: u64,
    held: u64,
    locked: bool,
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
struct PaymentProcessor {
    clients: HashMap<u16, Client>,
    processed_transactions: HashMap<u32, Transaction>,
    disputed_transactions: HashMap<u32, Dispute>,
}

impl PaymentProcessor {
    fn process_transaction(&mut self, transaction: Transaction) {
        let client_id = transaction.client();
        let client = self.clients.entry(client_id).or_insert(Client::default());
        match transaction {
            Transaction::Deposit { amount, tx, .. } => {
                if !client.locked {
                    client.available += amount;
                    self.processed_transactions.insert(tx, transaction);
                }
            }
            Transaction::Withdrawal { amount, tx, .. } => {
                if !client.locked && client.available >= amount {
                    client.available -= amount;
                    self.processed_transactions.insert(tx, transaction);
                }
            }
            Transaction::Dispute { tx, .. } => {
                // transaction is already disputed
                if self.disputed_transactions.get(&tx).is_some() {
                    return;
                }
                // transaction does not exists
                let Some(disputed_transaction) = self.processed_transactions.get(&tx) else {
                    return;
                };
                // the transaction in question was not a deposit
                let Some(deposit_amount) = disputed_transaction.deposit_amount() else {
                    return;
                };
                // client does not have the funds to dispute stuff
                if client.available >= deposit_amount {
                    client.available -= deposit_amount;
                    client.held += deposit_amount;
                    let dispute = Dispute::new(disputed_transaction.clone());
                    self.disputed_transactions.insert(tx, dispute);
                }
            }
            Transaction::Resolve { tx, .. } => {
                let Some(Dispute {
                    transaction,
                    status,
                }) = self.disputed_transactions.get_mut(&tx)
                else {
                    return;
                };
                if let DisputeStatus::Pending = status
                    && transaction.client() == client_id
                {
                    // we now that this is a deposit
                    let amount = transaction.deposit_amount().unwrap();
                    // we can remove this safely as a dispute should have increased this
                    client.held -= amount;
                    client.available += amount;
                    *status = DisputeStatus::Resolved;
                }
            }
            Transaction::Chargeback { tx, .. } => {
                let Some(Dispute {
                    transaction,
                    status,
                }) = self.disputed_transactions.get_mut(&tx)
                else {
                    return;
                };
                if let DisputeStatus::Pending = status
                    && transaction.client() == client_id
                {
                    let amount = transaction.deposit_amount().unwrap();
                    client.held -= amount;
                    client.locked = true;
                    *status = DisputeStatus::Chargedback;
                }
            }
        }
    }

    fn process_all_transactions(&mut self, transactions: Vec<Transaction>) {
        for trx in transactions {
            self.process_transaction(trx);
        }
    }

    fn report(&self) -> anyhow::Result<()> {
        // This will surely break on non unix things
        let stdout = PathBuf::from("/dev/stdout");
        let mut writer = WriterBuilder::new().from_path(stdout)?;
        let csv_output = self.clients.iter().map(|(id, client)| CsvOutputRow {
            client: *id,
            available: amount_to_str(client.available),
            held: amount_to_str(client.held),
            total: amount_to_str(client.available + client.held),
            locked: client.locked,
        });
        for output in csv_output {
            writer.serialize(output)?;
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct CsvOutputRow {
    client: u16,
    available: String,
    held: String,
    total: String,
    locked: bool,
}

fn main() -> anyhow::Result<()> {
    let input: Vec<_> = std::env::args().collect();
    let file = match input.as_slice() {
        [_, input] => input.clone(),
        [_, args @ ..] | [args @ ..] => {
            let args = args.to_vec();
            return Err(IncorrectAmountOfArgs(args).into());
        }
    };
    let mut file_path = current_dir()?;
    file_path.push(file);
    let mut reader = ReaderBuilder::new().trim(Trim::All).from_path(file_path)?;
    let records: Vec<CsvInputRow> = reader
        .deserialize::<CsvInputRow>()
        .collect::<Result<_, _>>()?;
    let transactions: Vec<Transaction> = records
        .into_iter()
        .map(|r| Transaction::try_from(r))
        .collect::<Result<_, _>>()?;
    let mut payment_processor = PaymentProcessor::default();
    payment_processor.process_all_transactions(transactions);
    payment_processor.report()?;
    Ok(())
}
