use crate::{
    PaymentError::{self, TransactionCouldNotBeParsed},
    TransactionCsvRow,
};

fn parse_transaction_amount(amount: &str) -> Result<i64, PaymentError> {
    if !amount.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return Err(PaymentError::invalid_amount(
            amount,
            "expected decimal digits and an optional decimal point",
        ));
    }
    let (whole, fraction) = amount.split_once('.').unwrap_or((amount, "0"));
    let fraction_digits = fraction.len();
    if fraction_digits > 4 {
        return Err(PaymentError::invalid_amount(
            amount,
            "at most four decimal places are allowed",
        ));
    }
    let whole: i64 = whole.parse().map_err(|err| {
        PaymentError::invalid_amount(amount, format!("invalid whole part {whole:?}: {err}"))
    })?;
    let fraction: i64 = fraction.parse().map_err(|err| {
        PaymentError::invalid_amount(
            amount,
            format!("invalid fractional part {fraction:?}: {err}"),
        )
    })?;
    let fraction = fraction * [10_000, 1_000, 100, 10, 1][fraction_digits];
    whole
        .checked_mul(10_000)
        .and_then(|whole| whole.checked_add(fraction))
        .ok_or_else(|| {
            PaymentError::invalid_amount(
                amount,
                "amount exceeds the supported range at four-decimal precision",
            )
        })
}

#[derive(Debug, Clone)]
pub enum TransactionKind {
    Deposit { amount: i64 },
    Withdrawal { amount: i64 },
    Dispute,
    Resolve,
    Chargeback,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DisputeStatus {
    Undisputed,
    Disputed,
    ChargedBack,
}

#[derive(Debug, Clone)]
pub struct Transaction {
    client: u16,
    tx: u32,
    kind: TransactionKind,
    status: DisputeStatus,
}

impl TryFrom<TransactionCsvRow> for Transaction {
    type Error = PaymentError;

    fn try_from(value: TransactionCsvRow) -> Result<Self, Self::Error> {
        let TransactionCsvRow { client, tx, .. } = value;
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
                let error = format!("Unrecognized transaction type: {type} with amount: {value:?}");
                Err(TransactionCouldNotBeParsed(error))
            }
        }
    }
}

impl From<&Transaction> for TransactionCsvRow {
    fn from(transaction: &Transaction) -> Self {
        let (kind, amount) = match transaction.kind {
            TransactionKind::Deposit { amount } => ("deposit", Some(amount)),
            TransactionKind::Withdrawal { amount } => ("withdrawal", Some(amount)),
            TransactionKind::Dispute => ("dispute", None),
            TransactionKind::Resolve => ("resolve", None),
            TransactionKind::Chargeback => ("chargeback", None),
        };
        let amount = amount.map(|amount| format!("{}.{:04}", amount / 10_000, amount % 10_000));
        Self {
            r#type: kind.into(),
            client: transaction.client,
            tx: transaction.tx,
            amount,
        }
    }
}

impl Transaction {
    /// # Errors
    /// Returns `PaymentError::InvalidAmount` if the amount is negative.
    pub fn deposit(client: u16, tx: u32, amount: i64) -> Result<Self, PaymentError> {
        if amount < 0 {
            return Err(PaymentError::invalid_amount(
                amount,
                "amount must be nonnegative",
            ));
        }
        Ok(Self {
            client,
            tx,
            kind: TransactionKind::Deposit { amount },
            status: DisputeStatus::Undisputed,
        })
    }

    /// # Errors
    /// Returns `PaymentError::InvalidAmount` if the amount is negative.
    pub fn withdrawal(client: u16, tx: u32, amount: i64) -> Result<Self, PaymentError> {
        if amount < 0 {
            return Err(PaymentError::invalid_amount(
                amount,
                "amount must be nonnegative",
            ));
        }
        Ok(Self {
            client,
            tx,
            kind: TransactionKind::Withdrawal { amount },
            status: DisputeStatus::Undisputed,
        })
    }

    #[must_use]
    pub fn dispute(client: u16, tx: u32) -> Self {
        Self {
            client,
            tx,
            kind: TransactionKind::Dispute,
            status: DisputeStatus::Undisputed,
        }
    }

    #[must_use]
    pub fn resolve(client: u16, tx: u32) -> Self {
        Self {
            client,
            tx,
            kind: TransactionKind::Resolve,
            status: DisputeStatus::Undisputed,
        }
    }

    #[must_use]
    pub fn chargeback(client: u16, tx: u32) -> Self {
        Self {
            client,
            tx,
            kind: TransactionKind::Chargeback,
            status: DisputeStatus::Undisputed,
        }
    }

    #[must_use]
    pub fn deposit_amount(&self) -> Option<i64> {
        match self.kind {
            TransactionKind::Deposit { amount } => Some(amount),
            _ => None,
        }
    }

    #[must_use]
    pub fn client_id(&self) -> u16 {
        self.client
    }

    #[must_use]
    pub fn transaction_id(&self) -> u32 {
        self.tx
    }

    #[must_use]
    pub fn kind(&self) -> &TransactionKind {
        &self.kind
    }

    pub(crate) fn disputable_amount(&self, client_id: u16) -> Option<i64> {
        if self.client == client_id && self.status == DisputeStatus::Undisputed {
            self.deposit_amount()
        } else {
            None
        }
    }

    pub(crate) fn disputed_amount(&self, client_id: u16) -> Option<i64> {
        if self.client == client_id && self.status == DisputeStatus::Disputed {
            self.deposit_amount()
        } else {
            None
        }
    }

    pub(crate) fn set_status(&mut self, status: DisputeStatus) {
        self.status = status;
    }
}
