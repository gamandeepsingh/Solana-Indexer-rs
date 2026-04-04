mod config;
mod db;
mod grpc;
mod models;
mod processor;
mod workers;

use colored::Colorize;
use config::Config;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("error")),
        )
        .init();

    let config = Config::from_env();

    println!("{} Connecting to database...", "→".dimmed());
    let db_pool = db::connection::init_db(&config.database_url).await;
    println!("{} Database connected", "✓".green().bold());

    println!(
        "{} Connecting to {}",
        "→".dimmed(),
        config.grpc_endpoint.cyan()
    );
    let channel = grpc::client::connect(&config.grpc_endpoint).await;
    println!("{} gRPC connected\n", "✓".green().bold());

    println!("{}", "  Streaming live Solana transactions...".bold());
    println!("{}", "─".repeat(60).dimmed());

    let (sender, receiver) = workers::queue::create_queue();
    let worker = tokio::spawn(workers::queue::start_worker(
        receiver,
        db_pool.clone(),
        config.console_log,
    ));

    tokio::select! {
        _ = grpc::stream::start_stream(channel, sender, db_pool, config.x_token) => {}
        _ = tokio::signal::ctrl_c() => {
            println!("\n{} Shutting down gracefully...", "[INFO]".blue().bold());
        }
    }

    // sender is dropped here — worker drains remaining txs then exits
    worker.await.ok();
    println!("{} Goodbye!", "[INFO]".blue().bold());
}
