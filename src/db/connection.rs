use std::time::Duration;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

pub async fn init_db(url: &str) -> PgPool {
    PgPoolOptions::new()
        .max_connections(10)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .idle_timeout(Duration::from_secs(60))
        .connect(url)
        .await
        .expect("Failed to connect to database")
}
