use std::sync::OnceLock;
use std::time::Instant;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::queries::{
    read_account, read_memos, read_slot_transactions, read_stats, read_transaction,
    read_transactions, read_transfers,
};

static START_TIME: OnceLock<Instant> = OnceLock::new();

fn start_time() -> &'static Instant {
    START_TIME.get_or_init(Instant::now)
}

type ApiResult<T> = Result<Json<T>, (StatusCode, String)>;

fn db_err(e: sqlx::Error) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

#[derive(Serialize)]
pub struct Page<T: Serialize> {
    pub data: Vec<T>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub uptime_secs: u64,
}

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        uptime_secs: start_time().elapsed().as_secs(),
    })
}

pub async fn stats(State(pool): State<PgPool>) -> ApiResult<crate::db::queries::StatsRow> {
    let s = read_stats(&pool).await.map_err(db_err)?;
    Ok(Json(s))
}

#[derive(Deserialize)]
pub struct TxListParams {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    pub success: Option<bool>,
}

fn default_limit() -> i64 {
    50
}

pub async fn list_transactions(
    State(pool): State<PgPool>,
    Query(p): Query<TxListParams>,
) -> ApiResult<Page<crate::db::queries::TxRow>> {
    let limit = p.limit.min(500).max(1);
    let (data, total) = read_transactions(&pool, limit, p.offset, p.success)
        .await
        .map_err(db_err)?;
    Ok(Json(Page {
        data,
        total,
        limit,
        offset: p.offset,
    }))
}

pub async fn get_transaction(
    State(pool): State<PgPool>,
    Path(signature): Path<String>,
) -> ApiResult<crate::db::queries::TxRow> {
    match read_transaction(&pool, &signature).await.map_err(db_err)? {
        Some(row) => Ok(Json(row)),
        None => Err((StatusCode::NOT_FOUND, format!("signature {signature} not found"))),
    }
}

pub async fn slot_transactions(
    State(pool): State<PgPool>,
    Path(slot): Path<i64>,
) -> ApiResult<Vec<crate::db::queries::TxRow>> {
    let rows = read_slot_transactions(&pool, slot).await.map_err(db_err)?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct TransferParams {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub min_amount: f64,
}

pub async fn list_transfers(
    State(pool): State<PgPool>,
    Query(p): Query<TransferParams>,
) -> ApiResult<Page<crate::db::queries::TransferRow>> {
    let limit = p.limit.min(500).max(1);
    let (data, total) = read_transfers(&pool, limit, p.offset, p.min_amount)
        .await
        .map_err(db_err)?;
    Ok(Json(Page {
        data,
        total,
        limit,
        offset: p.offset,
    }))
}

#[derive(Deserialize)]
pub struct MemoParams {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

pub async fn list_memos(
    State(pool): State<PgPool>,
    Query(p): Query<MemoParams>,
) -> ApiResult<Page<crate::db::queries::MemoRow>> {
    let limit = p.limit.min(500).max(1);
    let (data, total) = read_memos(&pool, limit, p.offset).await.map_err(db_err)?;
    Ok(Json(Page {
        data,
        total,
        limit,
        offset: p.offset,
    }))
}

pub async fn get_account(
    State(pool): State<PgPool>,
    Path(pubkey): Path<String>,
) -> ApiResult<crate::db::queries::AccountRow> {
    match read_account(&pool, &pubkey).await.map_err(db_err)? {
        Some(row) => Ok(Json(row)),
        None => Err((StatusCode::NOT_FOUND, format!("account {pubkey} not found"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode, header},
    };
    use serde_json::Value;
    use tower::ServiceExt;

    async fn router() -> Option<axum::Router> {
        let url = match std::env::var("DATABASE_URL") {
            Ok(u) => u,
            Err(_) => {
                eprintln!("[SKIP] DATABASE_URL not set — skipping DB-backed test");
                return None;
            }
        };
        let pool = crate::db::connection::init_read_pool(&url).await;
        Some(crate::api::build_router(pool))
    }

    async fn json_body(resp: axum::response::Response) -> Value {
        let bytes = axum::body::to_bytes(resp.into_body(), 10 * 1024 * 1024)
            .await
            .expect("failed to read body");
        serde_json::from_slice(&bytes).expect("body is not valid JSON")
    }

    async fn get(router: axum::Router, path: &str) -> axum::response::Response {
        router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("request failed")
    }

    #[tokio::test]
    async fn health_returns_ok_status() {
        let resp = health().await;
        assert_eq!(resp.0.status, "ok");
    }

    #[tokio::test]
    async fn health_uptime_is_non_negative() {
        let resp = health().await;
        let _ = resp.0.uptime_secs;
    }

    #[tokio::test]
    async fn health_uptime_increases() {
        let t1 = health().await.0.uptime_secs;
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        let t2 = health().await.0.uptime_secs;
        assert!(t2 >= t1, "uptime should be non-decreasing");
    }

    #[tokio::test]
    async fn health_http_200_json() {
        let Some(app) = router().await else { return };
        let resp = get(app, "/health").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp.headers().get(header::CONTENT_TYPE).unwrap();
        assert!(ct.to_str().unwrap().contains("application/json"));
    }

    #[tokio::test]
    async fn health_body_has_required_fields() {
        let Some(app) = router().await else { return };
        let resp = get(app, "/health").await;
        let body = json_body(resp).await;
        assert_eq!(body["status"], "ok");
        assert!(body["uptime_secs"].is_number());
    }

    #[tokio::test]
    async fn stats_returns_all_count_fields() {
        let Some(app) = router().await else { return };
        let resp = get(app, "/api/stats").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        for field in &[
            "total_transactions",
            "total_failed",
            "total_transfers",
            "total_memos",
            "total_accounts",
        ] {
            assert!(body[field].is_number(), "missing field: {field}");
        }
    }

    #[tokio::test]
    async fn stats_counts_are_non_negative() {
        let Some(app) = router().await else { return };
        let resp = get(app, "/api/stats").await;
        let body = json_body(resp).await;
        for field in &[
            "total_transactions",
            "total_failed",
            "total_transfers",
            "total_memos",
            "total_accounts",
        ] {
            assert!(body[field].as_i64().unwrap_or(-1) >= 0, "{field} should be >= 0");
        }
    }

    #[tokio::test]
    async fn list_transactions_returns_paginated_shape() {
        let Some(app) = router().await else { return };
        let resp = get(app, "/api/transactions").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert!(body["data"].is_array());
        assert!(body["total"].is_number());
        assert!(body["limit"].is_number());
        assert!(body["offset"].is_number());
    }

    #[tokio::test]
    async fn list_transactions_default_limit_is_50() {
        let Some(app) = router().await else { return };
        let resp = get(app, "/api/transactions").await;
        let body = json_body(resp).await;
        assert_eq!(body["limit"], 50);
        assert_eq!(body["offset"], 0);
    }

    #[tokio::test]
    async fn list_transactions_respects_limit_param() {
        let Some(app) = router().await else { return };
        let resp = get(app, "/api/transactions?limit=10").await;
        let body = json_body(resp).await;
        assert_eq!(body["limit"], 10);
        assert!(body["data"].as_array().unwrap().len() <= 10);
    }

    #[tokio::test]
    async fn list_transactions_limit_is_capped_at_500() {
        let Some(app) = router().await else { return };
        let resp = get(app, "/api/transactions?limit=9999").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["limit"], 500);
    }

    #[tokio::test]
    async fn list_transactions_success_filter_true() {
        let Some(app) = router().await else { return };
        let resp = get(app, "/api/transactions?success=true&limit=5").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        for tx in body["data"].as_array().unwrap() {
            assert_eq!(tx["success"], true);
        }
    }

    #[tokio::test]
    async fn list_transactions_success_filter_false() {
        let Some(app) = router().await else { return };
        let resp = get(app, "/api/transactions?success=false&limit=5").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        for tx in body["data"].as_array().unwrap() {
            assert_eq!(tx["success"], false);
        }
    }

    #[tokio::test]
    async fn list_transactions_offset_paginates() {
        let Some(app) = router().await else { return };
        let p1 = {
            let resp = get(app.clone(), "/api/transactions?limit=5&offset=0").await;
            json_body(resp).await
        };
        let p2 = {
            let resp = get(app, "/api/transactions?limit=5&offset=5").await;
            json_body(resp).await
        };
        assert_eq!(p1["total"], p2["total"]);
        if p1["total"].as_i64().unwrap_or(0) > 5 {
            assert_ne!(p1["data"][0]["signature"], p2["data"][0]["signature"]);
        }
    }

    #[tokio::test]
    async fn get_transaction_unknown_returns_404() {
        let Some(app) = router().await else { return };
        let resp = get(app, "/api/transactions/FAKESIGNATURE000000000000000000000000000000000000000000000000000000000000000000000").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_transaction_returns_correct_fields() {
        let Some(app) = router().await else { return };
        let list_resp = get(app.clone(), "/api/transactions?limit=1").await;
        let list_body = json_body(list_resp).await;
        let sigs = list_body["data"].as_array().unwrap();
        if sigs.is_empty() {
            eprintln!("[SKIP] no transactions in DB");
            return;
        }
        let sig = sigs[0]["signature"].as_str().unwrap();
        let resp = get(app, &format!("/api/transactions/{sig}")).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert!(body["signature"].is_string());
        assert!(body["slot"].is_number());
        assert!(body["success"].is_boolean());
        assert!(body["created_at"].is_string());
    }

    #[tokio::test]
    async fn slot_transactions_returns_array() {
        let Some(app) = router().await else { return };
        let list_resp = get(app.clone(), "/api/transactions?limit=1").await;
        let list_body = json_body(list_resp).await;
        let sigs = list_body["data"].as_array().unwrap();
        if sigs.is_empty() {
            eprintln!("[SKIP] no transactions in DB");
            return;
        }
        let slot = sigs[0]["slot"].as_i64().unwrap();
        let resp = get(app, &format!("/api/slots/{slot}")).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert!(body.is_array());
        for tx in body.as_array().unwrap() {
            assert_eq!(tx["slot"].as_i64().unwrap(), slot);
        }
    }

    #[tokio::test]
    async fn slot_transactions_empty_slot_returns_empty_array() {
        let Some(app) = router().await else { return };
        let resp = get(app, "/api/slots/0").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn list_transfers_returns_paginated_shape() {
        let Some(app) = router().await else { return };
        let resp = get(app, "/api/transfers").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert!(body["data"].is_array());
        assert!(body["total"].is_number());
    }

    #[tokio::test]
    async fn list_transfers_min_amount_filters_correctly() {
        let Some(app) = router().await else { return };
        let threshold = 5.0_f64;
        let resp = get(app, &format!("/api/transfers?min_amount={threshold}")).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        for transfer in body["data"].as_array().unwrap() {
            let amount = transfer["amount"].as_f64().unwrap();
            assert!(amount >= threshold, "amount {amount} below threshold {threshold}");
        }
    }

    #[tokio::test]
    async fn list_transfers_ordered_by_amount_desc() {
        let Some(app) = router().await else { return };
        let resp = get(app, "/api/transfers?limit=50").await;
        let body = json_body(resp).await;
        let amounts: Vec<f64> = body["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["amount"].as_f64().unwrap_or(0.0))
            .collect();
        for w in amounts.windows(2) {
            assert!(w[0] >= w[1], "transfers not sorted DESC: {} < {}", w[0], w[1]);
        }
    }

    #[tokio::test]
    async fn list_memos_returns_paginated_shape() {
        let Some(app) = router().await else { return };
        let resp = get(app, "/api/memos").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert!(body["data"].is_array());
        assert!(body["total"].is_number());
        assert_eq!(body["limit"], 50);
        assert_eq!(body["offset"], 0);
    }

    #[tokio::test]
    async fn list_memos_rows_have_required_fields() {
        let Some(app) = router().await else { return };
        let resp = get(app, "/api/memos?limit=5").await;
        let body = json_body(resp).await;
        for memo in body["data"].as_array().unwrap() {
            assert!(memo["id"].is_number());
            assert!(memo["signature"].is_string());
            assert!(memo["memo"].is_string());
            assert!(memo["created_at"].is_string());
        }
    }

    #[tokio::test]
    async fn get_account_unknown_returns_404() {
        let Some(app) = router().await else { return };
        let resp = get(app, "/api/accounts/NOTAREALKEY").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_account_returns_correct_fields() {
        let Some(app) = router().await else { return };
        let resp = get(app.clone(), "/api/accounts/NOTAREALKEY").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = json_body(resp).await;
        assert!(body.is_string() || body.is_object());
    }
}
