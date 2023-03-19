use sqlx::PgPool;
use time::PrimitiveDateTime;
use uuid::Uuid;

use crate::bank_web::refunds::ResponseData;

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

pub async fn insert(pool: &PgPool, payment_id: Uuid, amount: i32) -> Result<ResponseData, sqlx::Error> {
    let pay = crate::bank::payments::get(pool, payment_id).await;
    match pay {
        Ok(x) => {
            let payment_fund = x.amount;
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
                        Err(sqlx::Error::Protocol(
                            "The amount is more than the refundable amount".to_string(),
                        ))
                    }
                }
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
                        Err(sqlx::Error::Protocol(
                            "The amount is more than the refundable amount".to_string(),
                        ))
                    }
                }
            }
        }
        Err(err) => Err(err),
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
        pub async fn new_test(pool: &PgPool) -> Result<Refund, sqlx::Error> {
            let payment = Payment::new_test(pool).await?;

            let id = insert(pool, payment.id, REFUND_AMOUNT).await?;

            get(pool, id.id).await
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
