#[derive(Debug, thiserror::Error)]
pub enum PaymentError {
    #[error("Cli must have exactly one arg supplied, got insted: {0:?}")]
    IncorrectAmountOfArgs(Vec<String>),

    #[error("Transactions cannot be parsed from CsvRow: {0:?}")]
    TransactionCouldNotBeParsed(String),

    #[error("Amount arithmetic overflow, underflow, or out-of-range conversion")]
    ArithmeticError,

    #[error("Invalid amount {amount:?}: {reason}")]
    InvalidAmount { amount: String, reason: String },
}

impl PaymentError {
    pub fn invalid_amount(amount: impl ToString, reason: impl Into<String>) -> Self {
        Self::InvalidAmount {
            amount: amount.to_string(),
            reason: reason.into(),
        }
    }
}
