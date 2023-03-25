## 1. Fixed concurrency bug in refund
- Error: it allows excessive refunds if they are made concurrently

### *How to Fix*
```rust
pub async fn checked_insert(
    pool: &PgPool,
    payment_id: Uuid,
    refund_amount: i32,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query!(
        r#"
          INSERT into refunds ( payment_id, amount )
          SELECT $1, $2
          WHERE EXISTS (
            SELECT ( t2.amount - SUM(t1.amount) ) 
            FROM refunds t1 
            JOIN payments t2 on t1.payment_id = t2.id 
            WHERE t1.payment_id = $1 
            GROUP BY t1.payment_id, t2.amount
            HAVING t2.amount - SUM(t1.amount) >= $2::integer
          ) OR (
            NOT EXISTS (
              SELECT * FROM refunds WHERE payment_id = $1
            )
            AND EXISTS (
              SELECT * FROM payments WHERE id = $1 AND amount >= $2
            )
          )
          RETURNING id
        "#,
        payment_id,
        refund_amount
    )
    .fetch_optional(pool)
    .await
    .map(|record| record.map(|r| r.id))
}
```

This `checked_insert` function inserts refund after evaluating whether or not the refund amount is legal.

## 2. Fixed dealing concurrency requests in payment
- Bug: It is coerced to hold repeatedly when the payments with the same card are requested.

### *How to Fix*
Made a macro to improve code readability.
This macro parse the payment_result from AccountsService and update the payment status as Declined or Failed when there is a error from the service.

```rust
macro_rules! check_and_reverse_payment_status {
    ($bank_web:ident, $payment_result:ident, $payment_id:ident, $card_number:ident, $amount:ident ) => {
        if let Err(err_str) = $payment_result {
            let payment_err = PaymentError::from(&err_str);
            // update payment status to Declined or Failed, according to the payment_err type
            payments::update(
                &$bank_web.pool,
                $payment_id,
                payment_err.get_payment_status(),
            )
            .await
            .unwrap();
            return Ok((
                payment_err.get_http_status_code(),
                Json(ResponseBody::new(
                    Uuid::new_v4(),
                    $amount,
                    $card_number,
                    payment_err.get_payment_status(),
                )),
            ));
        }
    };
}
```
I removed concurrency bug by modifying the function architecture.
<br>
```javascript
1. Check Card Format
2. Insert Processing Payment to payments table
  It will fail if you use same card_number. (It will prevent from concurrent requests with the same card_number.)
3. Place hold in accounts service
4. If the holding is failed, it updates the Payment status as Failed in table
5. Else, Update the Payment status as Approved
6. Withdraw funds in accounts service
7. If the withdrawing is failed, it updates the Payment status as Failed in table and also it releases holding
```
```rust
pub async fn post<T: AccountService>(
    State(bank_web): State<BankWeb<T>>,
    Json(body): Json<RequestBody>,
) -> Result<(StatusCode, Json<ResponseBody>), (StatusCode, Json<ErrorResponseBody>)> {
    let amount = body.payment.amount;
    let card_number = body.payment.card_number.to_string();

    // payment requests for 0 should return a 204 response
    if amount == 0 {
        return Err((
            StatusCode::NO_CONTENT,
            Json(ErrorResponseBody::new("Amount shouldn't be 0")),
        ));
    }

    // payment requests for negative amounts should return a 400 response
    if amount < 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseBody::new("Amount shouldn't be negative")),
        ));
    }

    // invalid card formats should return a 422 response
    let card = match Card::try_from(card_number.clone()) {
        Ok(c) => c,
        Err(_e) => {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ErrorResponseBody::new("Bad Card Number format")),
            ))
        }
    };

    // insert Processing Payment
    let payment_id = unwrap_or_return!(
        payments::insert(
            &bank_web.pool,
            body.payment.amount,
            body.payment.card_number,
            payments::Status::Processing
        )
        .await,
        Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorResponseBody::new("card_number already used")),
        ))
    );
    // place hold
    let payment_result = bank_web
        .account_service
        .place_hold(card.account_number(), body.payment.amount)
        .await;

    // deal with payment_result
    check_and_reverse_payment_status!(bank_web, payment_result, payment_id, card_number, amount);

    payments::update(&bank_web.pool, payment_id, payments::Status::Approved)
        .await
        .unwrap();
    let payment_result = bank_web
        .account_service
        .withdraw_funds(payment_result.unwrap())
        .await;

    // deal with payment_result
    check_and_reverse_payment_status!(bank_web, payment_result, payment_id, card_number, amount);

    Ok((
        StatusCode::CREATED,
        Json(ResponseBody::new(
            payment_id,
            amount,
            card_number,
            payments::Status::Approved,
        )),
    ))
}
```
## 3. Fixed `service_unavailable`
### *How to fix*
Added `service_unavailable` in match statement
```rust
impl PaymentError {
    pub fn from(messages: &str) -> PaymentError {
        let (code, message) = match messages {
            ...
            "service_unavailable" => (503, "Service unavailable"),
            _ => (500, "Internal Error"),
```

## 4. Fixed `A successful payment should withdraw funds`
### *How to fix*
Made a call of `withdraw_funds` function after updating the Payment Status as `Approved`.
```rust
    let payment_result = bank_web
        .account_service
        .withdraw_funds(payment_result.unwrap())
        .await;
```

## 5. Fixed `Unnecessary requests aren't made to the accounts service for a negative payment amount`
### *How to fix*
Added check statements in post func in `payments.rs`
```rust
    // payment requests for negative amounts should return a 400 response
    if amount < 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseBody::new("Amount shouldn't be negative")),
        ));
    }
```
