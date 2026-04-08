mod api;
mod config;
mod db;
mod grpc;
mod metrics;
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
    let (write_pool, read_pool) = tokio::join!(
        db::connection::init_write_pool(&config.database_url),
        db::connection::init_read_pool(&config.database_url),
    );
    println!("{} Database connected", "✓".green().bold());

    println!(
        "{} Connecting to {}",
        "→".dimmed(),
        config.grpc_endpoint.cyan()
    );
    let channel = grpc::client::connect(&config.grpc_endpoint).await;
    println!("{} gRPC connected\n", "✓".green().bold());

    println!("{}", "  Streaming live Solana transactions...".bold());
    println!(
        "{} Benchmark logging → {}",
        "→".dimmed(),
        config.bench_log.cyan()
    );
    println!("{}", "─".repeat(60).dimmed());

    let m = metrics::Metrics::new();

    let (tx_sender, tx_receiver) = workers::queue::create_queue();
    let (acct_sender, acct_receiver) = workers::queue::create_acct_queue();

    let tx_worker = tokio::spawn(workers::queue::start_worker(
        tx_receiver,
        write_pool.clone(),
        config.console_log,
        m.clone(),
    ));
    let acct_worker = tokio::spawn(workers::queue::start_account_worker(
        acct_receiver,
        write_pool.clone(),
        m.clone(),
    ));
    let reporter = tokio::spawn(metrics::start_reporter(m.clone(), 300, config.bench_log));
    let api_port = config.api_port;

    tokio::select! {
        _ = grpc::stream::start_stream(channel, tx_sender, acct_sender, config.x_token, m) => {}
        _ = api::serve(read_pool, api_port) => {}
        _ = tokio::signal::ctrl_c() => {
            println!("\n{} Shutting down gracefully...", "[INFO]".blue().bold());
        }
    }

    reporter.abort();
    tx_worker.await.ok();
    acct_worker.await.ok();
    println!("{} Goodbye!", "[INFO]".blue().bold());
}
