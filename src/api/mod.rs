pub mod handlers;

use axum::{Router, routing::get};
use sqlx::PgPool;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use handlers::*;

pub fn build_router(pool: PgPool) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/stats", get(stats))
        .route("/api/transactions", get(list_transactions))
        .route("/api/transactions/{signature}", get(get_transaction))
        .route("/api/slots/{slot}", get(slot_transactions))
        .route("/api/transfers", get(list_transfers))
        .route("/api/memos", get(list_memos))
        .route("/api/accounts/{pubkey}", get(get_account))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(pool)
}

pub async fn serve(pool: PgPool, port: u16) {
    let app = build_router(pool);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    println!("→ API server on http://0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind API port");
    axum::serve(listener, app).await.expect("API server error");
}
