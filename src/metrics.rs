use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

pub struct Metrics {
    pub tx_received: AtomicU64,
    pub tx_written: AtomicU64,
    pub tx_failed: AtomicU64,
    pub tx_memo: AtomicU64,
    pub tx_transfer: AtomicU64,
    pub accounts_written: AtomicU64,
    pub slots: AtomicU64,
    pub batches: AtomicU64,
}

impl Metrics {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            tx_received: AtomicU64::new(0),
            tx_written: AtomicU64::new(0),
            tx_failed: AtomicU64::new(0),
            tx_memo: AtomicU64::new(0),
            tx_transfer: AtomicU64::new(0),
            accounts_written: AtomicU64::new(0),
            slots: AtomicU64::new(0),
            batches: AtomicU64::new(0),
        })
    }
}

pub async fn start_reporter(metrics: Arc<Metrics>, interval_secs: u64, log_path: String) {
    let mut file = match OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .await
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[ERROR] Cannot open bench log {log_path}: {e}");
            return;
        }
    };

    let header = format!(
        "\n--- session started {} ---\n\
         timestamp            | tps   | recv      | written   | failed    | failed% | memos | xfers | accts     | batches | slots\n\
         {}\n",
        Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
        "-".repeat(110),
    );
    let _ = file.write_all(header.as_bytes()).await;

    let mut prev_processed: u64 = 0;
    let mut timer = tokio::time::interval(Duration::from_secs(interval_secs));
    timer.tick().await;

    loop {
        timer.tick().await;

        let received = metrics.tx_received.load(Ordering::Relaxed);
        let written = metrics.tx_written.load(Ordering::Relaxed);
        let failed = metrics.tx_failed.load(Ordering::Relaxed);
        let memo = metrics.tx_memo.load(Ordering::Relaxed);
        let transfer = metrics.tx_transfer.load(Ordering::Relaxed);
        let accounts = metrics.accounts_written.load(Ordering::Relaxed);
        let slots = metrics.slots.load(Ordering::Relaxed);
        let batches = metrics.batches.load(Ordering::Relaxed);

        let processed = written + failed;
        let tps = processed.saturating_sub(prev_processed) / interval_secs;
        prev_processed = processed;

        let total = written + failed;
        let failed_pct = if total > 0 { (failed * 100) / total } else { 0 };

        let line = format!(
            "{} | {:>5} | {:>9} | {:>9} | {:>9} | {:>6}% | {:>5} | {:>5} | {:>9} | {:>7} | {:>5}\n",
            Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
            tps,
            received,
            written,
            failed,
            failed_pct,
            memo,
            transfer,
            accounts,
            batches,
            slots,
        );

        if let Err(e) = file.write_all(line.as_bytes()).await {
            eprintln!("[ERROR] bench log write: {e}");
        }
    }
}
