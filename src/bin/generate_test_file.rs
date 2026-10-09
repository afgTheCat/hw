use std::path::Path;

use anyhow::ensure;
use payments_engine::{CsvInputRow, PaymentProcessor, Transaction, TransactionType};
use rand::{Rng, SeedableRng, rngs::StdRng, seq::IteratorRandom};

const ROWS: usize = 1_000_000;
const ACTIVE_CLIENTS: u16 = 100;
const DEFAULT_SEED: u64 = 1;
const MISSING_TX: u32 = u32::MAX;

fn record(
    processor: &mut PaymentProcessor,
    transactions: &mut Vec<Transaction>,
    transaction: Transaction,
) -> anyhow::Result<()> {
    processor.process_transaction(transaction.clone())?;
    transactions.push(transaction);
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let seed = args
        .next()
        .map(|seed| seed.parse::<u64>())
        .transpose()?
        .unwrap_or(DEFAULT_SEED);
    ensure!(args.next().is_none(), "usage: generate_test_file [seed]");
    let mut rng = StdRng::seed_from_u64(seed);
    let mut processor = PaymentProcessor::default();
    let mut transactions = Vec::with_capacity(ROWS);
    let mut active: Vec<u16> = (1..=ACTIVE_CLIENTS).collect();
    let mut frozen = Vec::new();
    let mut last_client = ACTIVE_CLIENTS;
    // Each pool contains (client, transaction ID), referencing the engine's transactions.
    let mut deposits = Vec::new();
    let mut pending = Vec::new();

    for client in 1..=ACTIVE_CLIENTS {
        let tx = u32::from(client);
        record(
            &mut processor,
            &mut transactions,
            Transaction::deposit(client, tx, 10_000_000)?,
        )?;
        deposits.push((client, tx));
    }

    while transactions.len() < ROWS {
        let client = if !frozen.is_empty() && rng.random_bool(0.05) {
            frozen[rng.random_range(0..frozen.len())]
        } else {
            active[rng.random_range(0..active.len())]
        };
        let tx = u32::try_from(transactions.len() + 1)?;
        let roll = rng.random_range(0..1_000_000);
        let transaction = match roll {
            // 55% deposits, 44% withdrawals, 0.7% disputes, 0.2947% resolves,
            // and 0.0053% chargebacks: about 50 successful freezes per million rows.
            0..550_000 => {
                let transaction =
                    Transaction::deposit(client, tx, rng.random_range(1..=5_000_000))?;
                if !processor.client(client).unwrap().locked {
                    deposits.push((client, tx));
                }
                transaction
            }
            550_000..990_000 => {
                Transaction::withdrawal(client, tx, rng.random_range(1..=4_000_000))?
            }
            990_000..997_000 => {
                if !deposits.is_empty() && rng.random_bool(0.95) {
                    let reference = deposits.swap_remove(rng.random_range(0..deposits.len()));
                    pending.push(reference);
                    Transaction::dispute(reference.0, reference.1)
                } else {
                    Transaction::dispute(client, MISSING_TX)
                }
            }
            997_000..999_947 => {
                if !pending.is_empty() && rng.random_bool(0.95) {
                    let reference = pending.swap_remove(rng.random_range(0..pending.len()));
                    deposits.push(reference);
                    Transaction::resolve(reference.0, reference.1)
                } else {
                    Transaction::resolve(client, MISSING_TX)
                }
            }
            _ if transactions.len() + 4 <= ROWS => {
                let index = if rng.random_bool(0.95) {
                    pending
                        .iter()
                        .enumerate()
                        .filter(|(_, (client, _))| !processor.client(*client).unwrap().locked)
                        .map(|(index, _)| index)
                        .choose(&mut rng)
                } else {
                    None
                };
                if let Some(index) = index {
                    let (client, tx) = pending.swap_remove(index);
                    Transaction::chargeback(client, tx)
                } else {
                    Transaction::chargeback(client, MISSING_TX)
                }
            }
            // Leave room for the replacement and frozen-account attempts after a chargeback.
            _ => Transaction::withdrawal(client, tx, 10_000)?,
        };

        let client = transaction.client();
        let was_locked = processor.client(client).unwrap().locked;
        record(&mut processor, &mut transactions, transaction)?;
        if !was_locked && processor.client(client).unwrap().locked {
            frozen.push(client);
            last_client = last_client
                .checked_add(1)
                .expect("client ID space exhausted");
            let slot = active.iter().position(|id| *id == client).unwrap();
            active[slot] = last_client;
            let tx = u32::try_from(transactions.len() + 1)?;
            record(
                &mut processor,
                &mut transactions,
                Transaction::deposit(last_client, tx, 10_000_000)?,
            )?;
            deposits.push((last_client, tx));

            // Every frozen client attempts both operations; later rows also sample frozen clients.
            let tx = u32::try_from(transactions.len() + 1)?;
            record(
                &mut processor,
                &mut transactions,
                Transaction::deposit(client, tx, 10_000)?,
            )?;
            let tx = u32::try_from(transactions.len() + 1)?;
            record(
                &mut processor,
                &mut transactions,
                Transaction::withdrawal(client, tx, 10_000)?,
            )?;
        }
    }

    let active_count = (1..=last_client)
        .filter(|id| !processor.client(*id).unwrap().locked)
        .count();
    let counts = transactions
        .iter()
        .fold([0usize; 5], |mut counts, transaction| {
            let index = match transaction.kind() {
                TransactionType::Deposit { .. } => 0,
                TransactionType::Withdrawal { .. } => 1,
                TransactionType::Dispute => 2,
                TransactionType::Resolve => 3,
                TransactionType::Chargeback => 4,
            };
            counts[index] += 1;
            counts
        });
    eprintln!(
        "Generated {} rows with seed {seed}: {active_count} active, {} frozen, {last_client} total clients",
        transactions.len(),
        frozen.len()
    );
    eprintln!(
        "Deposits: {}, withdrawals: {}, disputes: {}, resolves: {}, chargebacks: {}",
        counts[0], counts[1], counts[2], counts[3], counts[4]
    );

    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    std::fs::create_dir_all(&assets)?;
    let path = assets.join(format!("test_1m_{seed}.csv"));
    let mut writer = csv::Writer::from_path(&path)?;
    for transaction in &transactions {
        writer.serialize(CsvInputRow::from(transaction))?;
    }
    writer.flush()?;
    eprintln!("Wrote {}", path.display());
    Ok(())
}
