use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use colored::Colorize;
use sqlx::PgPool;
use tokio::sync::mpsc::{self, Receiver, Sender, UnboundedReceiver, UnboundedSender};
use tokio::time::MissedTickBehavior;

use crate::db::queries::*;
use crate::metrics::Metrics;
use crate::models::account::Account;
use crate::models::transaction::Transaction;
use crate::processor;
use crate::processor::filters::LARGE_TRANSFER_THRESHOLD_SOL;

const BATCH_SIZE: usize = 200;
const FLUSH_INTERVAL_MS: u64 = 100;
const ACCT_BATCH_SIZE: usize = 200;
const ACCT_FLUSH_INTERVAL_MS: u64 = 2000;

pub fn create_queue() -> (UnboundedSender<Transaction>, UnboundedReceiver<Transaction>) {
    mpsc::unbounded_channel()
}

pub fn create_acct_queue() -> (Sender<Account>, Receiver<Account>) {
    mpsc::channel(10_000)
}

pub async fn start_worker(
    mut rx: UnboundedReceiver<Transaction>,
    db: PgPool,
    console_log: bool,
    metrics: Arc<Metrics>,
) {
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
                            flush(&mut buffer, &db, &metrics).await;
                        }
                    }
                    None => break,
                }
            }
            _ = timer.tick() => {
                if !buffer.is_empty() {
                    flush(&mut buffer, &db, &metrics).await;
                }
            }
        }
    }

    if !buffer.is_empty() {
        eprintln!(
            "{} Flushing {} remaining transactions...",
            "[INFO]".blue().bold(),
            buffer.len()
        );
        flush(&mut buffer, &db, &metrics).await;
    }
    eprintln!("{} Worker stopped", "[INFO]".blue().bold());
}

pub async fn start_account_worker(
    mut rx: Receiver<Account>,
    db: PgPool,
    metrics: Arc<Metrics>,
) {
    let mut buffer: Vec<Account> = Vec::with_capacity(ACCT_BATCH_SIZE);
    let mut timer = tokio::time::interval(Duration::from_millis(ACCT_FLUSH_INTERVAL_MS));
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;
            msg = rx.recv() => {
                match msg {
                    Some(acct) => {
                        buffer.push(acct);
                        if buffer.len() >= ACCT_BATCH_SIZE {
                            flush_accounts(&mut buffer, &db, &metrics).await;
                        }
                    }
                    None => break,
                }
            }
            _ = timer.tick() => {
                if !buffer.is_empty() {
                    flush_accounts(&mut buffer, &db, &metrics).await;
                }
            }
        }
    }

    if !buffer.is_empty() {
        flush_accounts(&mut buffer, &db, &metrics).await;
    }
    eprintln!("{} Account worker stopped", "[INFO]".blue().bold());
}

async fn flush(batch: &mut Vec<Transaction>, db: &PgPool, metrics: &Arc<Metrics>) {
    let written = batch.iter().filter(|t| !t.failed).count() as u64;
    let failed = batch.iter().filter(|t| t.failed).count() as u64;
    let memo = batch
        .iter()
        .filter(|t| t.memo.is_some() && !t.failed)
        .count() as u64;
    let transfer = batch
        .iter()
        .filter(|t| !t.failed && t.amount > LARGE_TRANSFER_THRESHOLD_SOL)
        .count() as u64;

    let results = tokio::join!(
        copy_transactions(db, batch),
        batch_insert_failed(db, batch),
        batch_insert_memos(db, batch),
        batch_insert_transfers(db, batch, LARGE_TRANSFER_THRESHOLD_SOL),
    );

    for (label, res) in [
        ("transactions (copy)", results.0),
        ("failed_transactions", results.1),
        ("memos", results.2),
        ("large_transfers", results.3),
    ] {
        if let Err(e) = res {
            eprintln!("{} batch {label}: {e}", "[ERROR]".red().bold());
        }
    }

    metrics.tx_written.fetch_add(written, Ordering::Relaxed);
    metrics.tx_failed.fetch_add(failed, Ordering::Relaxed);
    metrics.tx_memo.fetch_add(memo, Ordering::Relaxed);
    metrics.tx_transfer.fetch_add(transfer, Ordering::Relaxed);
    metrics.batches.fetch_add(1, Ordering::Relaxed);

    batch.clear();
}

async fn flush_accounts(batch: &mut Vec<Account>, db: &PgPool, metrics: &Arc<Metrics>) {
    let count = batch.len() as u64;
    if let Err(e) = batch_upsert_accounts(db, batch).await {
        eprintln!("{} batch accounts: {e}", "[ERROR]".red().bold());
    } else {
        metrics.accounts_written.fetch_add(count, Ordering::Relaxed);
    }
    batch.clear();
}
