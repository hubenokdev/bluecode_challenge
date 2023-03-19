use sqlx::PgPool;
use time::PrimitiveDateTime;
use uuid::Uuid;

use crate::bank_web::refunds::ResponseData;
use crate::errors::CustomError;

use super::payments::Status;

/// Module and schema representing a refund.
///
/// A refund is always tied to a specific payment record, but it is possible
/// to make partial refunds (i.e. refund less than the total payment amount).
/// In the same vein, it is possible to apply several refunds against the same
/// payment record, the but sum of all refunded amounts for a given payment can
/// never surpass the original payment amount.
///
/// If a refund is persisted in the database, it is considered effective: the
/// bank's client will have the money credited to their account.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Refund {
    pub id: Uuid,
    pub payment_id: Uuid,
    pub amount: i32,
    pub inserted_at: PrimitiveDateTime,
    pub updated_at: PrimitiveDateTime,
}

// Store the refund details in the database
pub async fn insert(
    pool: &PgPool,
    payment_id: Uuid,
    amount: i32,
) -> Result<ResponseData, CustomError> {
    // Gettting the payment details from payment table
    let pay = crate::bank::payments::get(pool, payment_id).await;
    // Checkking a valid payment is there if thre then
    match pay {
        Ok(x) => {
            let payment_fund = x.amount;
                if x.status == Status::Approved{
                // check any refund is there already claimed, if there check with the claimed refund amount and this amout with payment
                let refund = get_payment_refund(pool, x.id).await?;
                match refund {
                    Some(refund) => {
                        let total = refund.amount + amount;
                        if total <= payment_fund {
                            let s = sqlx::query!(
                                r#"
                                    UPDATE refunds SET amount = $1 WHERE payment_id =$2
                                    RETURNING *
                                "#,
                                total,
                                payment_id,
                            )
                            .fetch_one(pool)
                            .await?;
                            let res = ResponseData::new(s.id, s.payment_id, s.amount);
                            Ok(res)
                        } else {
                            Err(CustomError::AmoutRefundFailed {
                                message: "The amount is more than the refundable amount".to_string(),
                                code: 422,
                            })
                        }
                    }
                    // None of the refund claimed then insert a new refund
                    None => {
                        if payment_fund >= amount {
                            let query = sqlx::query!(
                                r#"
                                    INSERT INTO refunds ( payment_id, amount)
                                    VALUES ( $1, $2 )
                                    RETURNING *
                                "#,
                                payment_id,
                                amount,
                            )
                            .fetch_one(pool)
                            .await?;
                            let res = ResponseData::new(query.id, query.payment_id, query.amount);
                            Ok(res)
                        } else {
                            Err(CustomError::AmoutRefundFailed {
                                message: "The amount is more than the refundable amount".to_string(),
                                code: 422,
                            })
                        }
                    }
                }
            }else {
                Err(CustomError::PaymentNotExist {
                    code: 404,
                    message: format!("Failed to refund the amount "),
                })
            }
        }
        Err(err) => Err(CustomError::PaymentNotExist {
            code: 404,
            message: format!("Failed to refund the amount {}", err),
        }),
    }
}

pub async fn get(pool: &PgPool, id: Uuid) -> Result<Refund, sqlx::Error> {
    sqlx::query_as!(
        Refund,
        r#"
            SELECT id, payment_id, amount, inserted_at, updated_at FROM refunds
            WHERE id = $1
        "#,
        id
    )
    .fetch_one(pool)
    .await
}

// Query payment refund details from the database
pub async fn get_payment_refund(
    pool: &PgPool,
    payment_id: Uuid,
) -> Result<Option<Refund>, sqlx::Error> {
    sqlx::query_as!(
        Refund,
        r#"
            SELECT id, payment_id, amount, inserted_at, updated_at FROM refunds
            WHERE payment_id = $1
        "#,
        payment_id
    )
    .fetch_optional(pool)
    .await
}

#[cfg(test)]
pub mod tests {

    use super::*;
    use crate::bank::payments::Payment;

    pub const REFUND_AMOUNT: i32 = 42;

    impl Refund {
        pub async fn new_test(pool: &PgPool) -> Result<Refund, CustomError> {
            let payment = Payment::new_test(pool).await?;

            let id = insert(pool, payment.id, REFUND_AMOUNT).await?;

            match get(pool, id.id).await{
                Ok(x) => Ok(x),
                Err(e) => Err(CustomError::from(e)),
            }
        }
    }

    #[tokio::test]
    async fn test_refund() {
        let pool = crate::pg_pool()
            .await
            .expect("failed to connect to postgres");

        let refund = Refund::new_test(&pool)
            .await
            .expect("failed to create refund");

        assert_eq!(refund.amount, REFUND_AMOUNT);
    }
}
