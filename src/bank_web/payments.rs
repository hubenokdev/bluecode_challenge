use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{BankWeb, ErrorResponseBody};
use crate::bank::{accounts::AccountService, payments};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RequestData {
    pub amount: i32,
    pub card_number: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RequestBody {
    pub payment: RequestData,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ResponseData {
    pub id: Uuid,
    pub amount: i32,
    pub card_number: String,
    pub status: payments::Status,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ResponseBody {
    pub data: ResponseData,
}

pub async fn post<T: AccountService>(
    State(bank_web): State<BankWeb<T>>,
    Json(body): Json<RequestBody>,
) -> Result<(StatusCode, Json<ResponseBody>), (StatusCode, Json<ErrorResponseBody>)> {
    let payment_id: Uuid;

    if body.payment.amount == 0 {
        return Err((
            StatusCode::NO_CONTENT,
            Json(ErrorResponseBody {
                error: "zero amount".to_owned(),
            }),
        ));
    }

    let hold = bank_web
        .account_service
        .place_hold(&body.payment.card_number, body.payment.amount)
        .await;

    match hold {
        Ok(_) => {
            match payments::insert(
                &bank_web.pool,
                body.payment.amount,
                body.payment.card_number,
                payments::Status::Approved,
            )
            .await
            {
                Ok(some_payment_id) => {
                    bank_web
                        .account_service
                        .withdraw_funds(hold.unwrap())
                        .await
                        .unwrap();
                    payment_id = some_payment_id;
                    let payment = payments::get(&bank_web.pool, payment_id).await.unwrap();
                    return Ok((
                        StatusCode::CREATED,
                        Json(ResponseBody {
                            data: ResponseData {
                                id: payment.id,
                                amount: payment.amount,
                                card_number: payment.card_number,
                                status: payment.status,
                            },
                        }),
                    ));
                }
                Err(_) => {
                    bank_web
                        .account_service
                        .release_hold(hold.unwrap())
                        .await
                        .unwrap();
                    return Err((
                        StatusCode::UNPROCESSABLE_ENTITY,
                        Json(ErrorResponseBody {
                            error: "card_number already used".to_owned(),
                        }),
                    ));
                }
            }
        }
        Err(errmsg) => match errmsg.as_str() {
            "invalid_account_number" => {
                return Ok((
                    StatusCode::FORBIDDEN,
                    Json(ResponseBody {
                        data: ResponseData {
                            id: Uuid::nil(),
                            amount: body.payment.amount,
                            card_number: body.payment.card_number,
                            status: payments::Status::Declined,
                        },
                    }),
                ));
            }
            "invalid_amount" => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponseBody {
                        error: "invalid amount".to_owned(),
                    }),
                ))
            }
            "insufficient_funds" => {
                return Ok((
                    StatusCode::PAYMENT_REQUIRED,
                    Json(ResponseBody {
                        data: ResponseData {
                            id: Uuid::nil(),
                            amount: body.payment.amount,
                            card_number: body.payment.card_number,
                            status: payments::Status::Declined,
                        },
                    }),
                ));
            }
            _ => Err((
                StatusCode::NO_CONTENT,
                Json(ErrorResponseBody {
                    error: "cannot process the request".to_owned(),
                }),
            )),
        },
    }
}

pub async fn get<T: AccountService>(
    State(bank_web): State<BankWeb<T>>,
    Path(payment_id): Path<Uuid>,
) -> Result<(StatusCode, Json<ResponseBody>), (StatusCode, Json<ErrorResponseBody>)> {
    let payment = payments::get(&bank_web.pool, payment_id).await.unwrap();

    Ok((
        StatusCode::OK,
        Json(ResponseBody {
            data: ResponseData {
                id: payment.id,
                amount: payment.amount,
                card_number: payment.card_number,
                status: payment.status,
            },
        }),
    ))
}

#[cfg(test)]
pub mod tests {

    use super::*;
    use crate::{
        bank::{payment_instruments::Card, payments::Status},
        bank_web::tests::{deserialize_response_body, get, post},
    };

    #[tokio::test]
    async fn should_approve_valid_payment() {
        let router = BankWeb::new_test().await.into_router();

        let request_body = RequestBody {
            payment: RequestData {
                amount: 1205,
                card_number: Card::new_test().into(),
            },
        };

        let response = post(&router, "/api/payments", &request_body).await;
        assert_eq!(response.status(), 201);

        let response_body = deserialize_response_body::<ResponseBody>(response).await;
        assert_eq!(response_body.data.amount, request_body.payment.amount);

        let uri = format!("/api/payments/{}", response_body.data.id);
        let response = get(&router, uri).await;
        assert_eq!(response.status(), 200);

        let response_body = deserialize_response_body::<ResponseBody>(response).await;
        assert_eq!(response_body.data.amount, request_body.payment.amount);
        assert_eq!(response_body.data.status, Status::Approved);
    }

    #[tokio::test]
    async fn should_decline_payment_and_return_402_with_insufficient_funds() {
        let router = BankWeb::new_test_with_response("insufficient_funds")
            .await
            .into_router();

        let request_body = RequestBody {
            payment: RequestData {
                amount: 1205,
                card_number: Card::new_test().into(),
            },
        };

        let response = post(&router, "/api/payments", &request_body).await;
        assert_eq!(response.status(), 402);

        let response_body = deserialize_response_body::<ResponseBody>(response).await;
        assert_eq!(response_body.data.amount, request_body.payment.amount);
        assert_eq!(response_body.data.status, Status::Declined);
    }

    #[tokio::test]
    async fn should_decline_payment_and_return_403_for_invalid_account_number() {
        let router = BankWeb::new_test_with_response("invalid_account_number")
            .await
            .into_router();

        let request_body = RequestBody {
            payment: RequestData {
                amount: 1205,
                card_number: Card::new_test().into(),
            },
        };

        let response = post(&router, "/api/payments", &request_body).await;
        assert_eq!(response.status(), 403);

        let response_body = deserialize_response_body::<ResponseBody>(response).await;
        assert_eq!(response_body.data.amount, request_body.payment.amount);
        assert_eq!(response_body.data.status, Status::Declined);
    }

    #[tokio::test]
    async fn should_return_204_for_zero_amount() {
        let router = BankWeb::new_test().await.into_router();

        let request_body = RequestBody {
            payment: RequestData {
                amount: 0,
                card_number: Card::new_test().into(),
            },
        };

        let response = post(&router, "/api/payments", &request_body).await;
        assert_eq!(response.status(), 204);
    }

    #[tokio::test]
    async fn should_return_422_for_existing_card_number() {
        let router = BankWeb::new_test().await.into_router();

        let request_body = RequestBody {
            payment: RequestData {
                amount: 123,
                card_number: Card::new_test().into(),
            },
        };

        let response = post(&router, "/api/payments", &request_body).await;
        assert_eq!(response.status(), 201);

        let response = post(&router, "/api/payments", &request_body).await;
        assert_eq!(response.status(), 422);

        let response_body = deserialize_response_body::<ErrorResponseBody>(response).await;
        assert_eq!(response_body.error, "card_number already used");
    }
}
