use crate::{
    CsvInputRow,
    HwErrors::{self, *},
};

fn parse_transaction_amount(amount: &str) -> Result<i64, HwErrors> {
    if !amount.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return Err(HwErrors::invalid_amount(
            amount,
            "expected decimal digits and an optional decimal point",
        ));
    }
    let (whole, fraction) = amount.split_once('.').unwrap_or((amount, "0"));
    let fraction_digits = fraction.len();
    if fraction_digits > 4 {
        return Err(HwErrors::invalid_amount(
            amount,
            "at most four decimal places are allowed",
        ));
    }
    let whole: i64 = whole.parse().map_err(|err| {
        HwErrors::invalid_amount(amount, format!("invalid whole part {whole:?}: {err}"))
    })?;
    let fraction: i64 = fraction.parse().map_err(|err| {
        HwErrors::invalid_amount(
            amount,
            format!("invalid fractional part {fraction:?}: {err}"),
        )
    })?;
    let fraction = fraction * 10i64.pow(4 - fraction_digits as u32);
    whole
        .checked_mul(10_000)
        .and_then(|whole| whole.checked_add(fraction))
        .ok_or_else(|| {
            HwErrors::invalid_amount(
                amount,
                "amount exceeds the supported range at four-decimal precision",
            )
        })
}

#[derive(Debug, Clone)]
pub enum TransactionType {
    Deposit { amount: i64 },
    Withdrawal { amount: i64 },
    Dispute,
    Resolve,
    Chargeback,
}

#[derive(Debug, Clone)]
pub struct Transaction {
    client: u16,
    tx: u32,
    kind: TransactionType,
}

impl Transaction {
    pub fn deposit(client: u16, tx: u32, amount: i64) -> Result<Self, HwErrors> {
        if amount < 0 {
            return Err(HwErrors::invalid_amount(
                amount,
                "amount must be nonnegative",
            ));
        }
        Ok(Self {
            client,
            tx,
            kind: TransactionType::Deposit { amount },
        })
    }

    pub fn withdrawal(client: u16, tx: u32, amount: i64) -> Result<Self, HwErrors> {
        if amount < 0 {
            return Err(HwErrors::invalid_amount(
                amount,
                "amount must be nonnegative",
            ));
        }
        Ok(Self {
            client,
            tx,
            kind: TransactionType::Withdrawal { amount },
        })
    }

    pub fn dispute(client: u16, tx: u32) -> Self {
        Self {
            client,
            tx,
            kind: TransactionType::Dispute,
        }
    }

    pub fn resolve(client: u16, tx: u32) -> Self {
        Self {
            client,
            tx,
            kind: TransactionType::Resolve,
        }
    }

    pub fn chargeback(client: u16, tx: u32) -> Self {
        Self {
            client,
            tx,
            kind: TransactionType::Chargeback,
        }
    }

    pub fn deposit_amount(&self) -> Option<i64> {
        match self.kind {
            TransactionType::Deposit { amount } => Some(amount),
            _ => None,
        }
    }

    pub fn client(&self) -> u16 {
        self.client
    }

    pub fn tx(&self) -> u32 {
        self.tx
    }

    pub fn kind(&self) -> &TransactionType {
        &self.kind
    }
}

impl TryFrom<CsvInputRow> for Transaction {
    type Error = HwErrors;

    fn try_from(value: CsvInputRow) -> Result<Self, Self::Error> {
        let CsvInputRow { client, tx, .. } = value;
        match (value.r#type.as_str(), value.amount) {
            ("deposit", Some(amount)) => {
                let amount = parse_transaction_amount(&amount)?;
                Self::deposit(client, tx, amount)
            }
            ("withdrawal", Some(amount)) => {
                let amount = parse_transaction_amount(&amount)?;
                Self::withdrawal(client, tx, amount)
            }
            ("dispute", None) => Ok(Self::dispute(client, tx)),
            ("resolve", None) => Ok(Self::resolve(client, tx)),
            ("chargeback", None) => Ok(Self::chargeback(client, tx)),
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
