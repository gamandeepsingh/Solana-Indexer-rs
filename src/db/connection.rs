use std::time::Duration;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

pub async fn init_write_pool(url: &str) -> PgPool {
    PgPoolOptions::new()
        .max_connections(20)
        .min_connections(5)
        .acquire_timeout(Duration::from_secs(10))
        .idle_timeout(Duration::from_secs(60))
        .connect(url)
        .await
        .expect("Failed to connect write pool")
}

pub async fn init_read_pool(url: &str) -> PgPool {
    PgPoolOptions::new()
        .max_connections(20)
        .min_connections(5)
        .acquire_timeout(Duration::from_secs(10))
        .idle_timeout(Duration::from_secs(60))
        .connect(url)
        .await
        .expect("Failed to connect read pool")
}
