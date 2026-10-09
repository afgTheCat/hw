use crate::{
    AMOUNT_CAN_NOT_BE_PARSED, CsvInputRow,
    HwErrors::{self, *},
};

fn parse_amount(amount: &str) -> Result<i64, HwErrors> {
    let Some((amount, bp)) = amount.split_once(".") else {
        let err = "Amount without '.' separator".into();
        return Err(TransactionCouldNotBeParsed(err));
    };
    if amount.starts_with('-') || bp.starts_with('-') {
        return Err(NegativeAmount);
    }
    let bp_decimals = bp.len();
    if bp_decimals > 4 {
        let err = "Amount with higher precision than bp".into();
        return Err(TransactionCouldNotBeParsed(err));
    }
    let amount = i64::from_str_radix(amount, 10)
        .map_err(|_err| TransactionCouldNotBeParsed(AMOUNT_CAN_NOT_BE_PARSED.into()))?;
    let bp = i64::from_str_radix(bp, 10)
        .map_err(|_err| TransactionCouldNotBeParsed(AMOUNT_CAN_NOT_BE_PARSED.into()))?;
    let bp_decimals = u32::try_from(bp_decimals).map_err(|_| ArithmeticError)?;
    let exponent = 4u32.checked_sub(bp_decimals).ok_or(ArithmeticError)?;
    let scale = 10i64.checked_pow(exponent).ok_or(ArithmeticError)?;
    let bp = bp.checked_mul(scale).ok_or(ArithmeticError)?;
    amount
        .checked_mul(10_000)
        .and_then(|whole| whole.checked_add(bp))
        .ok_or(ArithmeticError)
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
            return Err(NegativeAmount);
        }
        Ok(Self {
            client,
            tx,
            kind: TransactionType::Deposit { amount },
        })
    }

    pub fn withdrawal(client: u16, tx: u32, amount: i64) -> Result<Self, HwErrors> {
        if amount < 0 {
            return Err(NegativeAmount);
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
                let amount = parse_amount(&amount)?;
                Self::deposit(client, tx, amount)
            }
            ("withdrawal", Some(amount)) => {
                let amount = parse_amount(&amount)?;
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
