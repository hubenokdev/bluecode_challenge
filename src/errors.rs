use std::fmt::Display;

use thiserror::Error;

use crate::bank::payment_instruments::CardError;

#[derive(Error, Debug)]
pub enum CustomError {
    #[error("Unauthorized")]
    Unauthorized {},

    #[error("{0}")]
    Sql(#[from] sqlx::Error),

    #[error("{0}")]
    CardError(#[from] CardError),

    // #[error("PaymentRequired {code} {message}")]
    // PaymentRequired { code: i32, message: String },
    // #[error("InvalidAmountInvalidAmount {code} {message}")]
    // InvalidAmount { code: i32, message: String },
    // #[error("PaymentRequired {code} {message}")]
    // ZeroAmoutTransfre { code: i32, message: String },
    #[error("InValidCard {code} {message}")]
    InValidCard { code: i32, message: String },

    #[error("AmoutExcRefundFailed {code} {message}")]
    AmoutRefundFailed { code: i32, message: String },

    #[error("PaymentNotExist {code} {message}")]
    PaymentNotExist { code: i32, message: String },

    #[error("Payment Error {0}")]
    PaymentError(PaymentError),
}

#[derive(Debug)]
pub struct PaymentError {
    pub code: i32,
    pub message: String,
}

impl Display for PaymentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl PaymentError {
    pub fn from(messages: &str) -> PaymentError {
        let (code, message) = match messages {
            "invalid_account_number" => (403, "Forbidden"),
            "invalid_amount" => (400, "Forbidden"),
            "insufficient_funds" => (402, " Payment Requires"),
            _ => (500, "Internal Error"),
        };
        PaymentError {
            code,
            message: message.to_string(),
        }
    }
}
