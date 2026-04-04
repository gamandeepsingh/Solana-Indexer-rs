use std::time::Duration;

use colored::Colorize;
use sqlx::PgPool;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::time::MissedTickBehavior;

use crate::db::queries::*;
use crate::models::transaction::Transaction;
use crate::processor;
use crate::processor::filters::LARGE_TRANSFER_THRESHOLD_SOL;

const BATCH_SIZE: usize = 50;
const FLUSH_INTERVAL_MS: u64 = 500;

pub fn create_queue() -> (Sender<Transaction>, Receiver<Transaction>) {
    mpsc::channel(1000)
}

pub async fn start_worker(mut rx: Receiver<Transaction>, db: PgPool, console_log: bool) {
    let mut buffer: Vec<Transaction> = Vec::with_capacity(BATCH_SIZE);
    let mut timer = tokio::time::interval(Duration::from_millis(FLUSH_INTERVAL_MS));
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;
            msg = rx.recv() => {
                match msg {
                    Some(tx) => {
                        processor::transaction::print_tx(&tx, console_log);
                        buffer.push(tx);
                        if buffer.len() >= BATCH_SIZE {
                            flush(&mut buffer, &db).await;
                        }
                    }
                    None => break, // sender dropped — graceful shutdown
                }
            }
            _ = timer.tick() => {
                if !buffer.is_empty() {
                    flush(&mut buffer, &db).await;
                }
            }
        }
    }

    // Drain whatever is left before exiting
    if !buffer.is_empty() {
        eprintln!(
            "{} Flushing {} remaining transactions...",
            "[INFO]".blue().bold(),
            buffer.len()
        );
        flush(&mut buffer, &db).await;
    }
    eprintln!("{} Worker stopped", "[INFO]".blue().bold());
}

async fn flush(batch: &mut Vec<Transaction>, db: &PgPool) {
    let results = tokio::join!(
        batch_insert_transactions(db, batch),
        batch_insert_failed(db, batch),
        batch_insert_memos(db, batch),
        batch_insert_transfers(db, batch, LARGE_TRANSFER_THRESHOLD_SOL),
    );

    for (label, res) in [
        ("transactions", results.0),
        ("failed_transactions", results.1),
        ("memos", results.2),
        ("large_transfers", results.3),
    ] {
        if let Err(e) = res {
            eprintln!("{} batch {label}: {e}", "[ERROR]".red().bold());
        }
    }

    batch.clear();
}
