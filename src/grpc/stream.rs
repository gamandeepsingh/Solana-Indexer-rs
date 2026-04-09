use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use colored::Colorize;
use tokio::sync::mpsc::{self, Sender, UnboundedSender};
use tokio_stream::StreamExt;
use tonic::{Request, Status, metadata::MetadataValue, transport::Channel};
use yellowstone_grpc_proto::geyser::{
    CommitmentLevel, SubscribeRequest, SubscribeRequestFilterSlots,
    SubscribeRequestFilterTransactions, SubscribeUpdateTransaction, SubscribeUpdateTransactionInfo,
    geyser_client::GeyserClient, subscribe_update::UpdateOneof,
};

use crate::metrics::Metrics;
use crate::models::account::Account;
use crate::models::transaction::Transaction;

const MEMO_V1: &str = "Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo";
const MEMO_V2: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";
const RECONNECT_BASE_SECS: u64 = 2;
const RECONNECT_MAX_SECS: u64 = 30;
const RECONNECT_RESET_SECS: u64 = 60;

pub async fn start_stream(
    channel: Channel,
    tx_queue: UnboundedSender<Transaction>,
    acct_queue: Sender<Account>,
    x_token: Option<String>,
    metrics: Arc<Metrics>,
) {
    let mut attempt: u32 = 0;

    loop {
        let started = tokio::time::Instant::now();

        run_once(
            channel.clone(),
            &tx_queue,
            &acct_queue,
            &x_token,
            &metrics,
        )
        .await;

        if started.elapsed().as_secs() >= RECONNECT_RESET_SECS {
            attempt = 0;
        }

        attempt += 1;
        let delay = (RECONNECT_BASE_SECS * 2u64.pow(attempt.min(4))).min(RECONNECT_MAX_SECS);

        eprintln!(
            "{} Stream ended — reconnecting in {}s (attempt {})...",
            "[WARN]".yellow().bold(),
            delay,
            attempt,
        );
        tokio::time::sleep(Duration::from_secs(delay)).await;
    }
}

async fn run_once(
    channel: Channel,
    tx_queue: &UnboundedSender<Transaction>,
    acct_queue: &Sender<Account>,
    x_token: &Option<String>,
    metrics: &Arc<Metrics>,
) {
    let (tx, rx) = mpsc::channel::<SubscribeRequest>(128);

    let request = SubscribeRequest {
        transactions: HashMap::from([(
            "all".to_string(),
            SubscribeRequestFilterTransactions {
                vote: Some(false),
                failed: None,
                ..Default::default()
            },
        )]),
        slots: HashMap::from([(
            "slots".to_string(),
            SubscribeRequestFilterSlots {
                filter_by_commitment: Some(false),
                interslot_updates: Some(false),
            },
        )]),
        commitment: Some(CommitmentLevel::Confirmed as i32),
        ..Default::default()
    };

    if tx.send(request).await.is_err() {
        eprintln!(
            "{} Failed to send subscribe request",
            "[ERROR]".red().bold()
        );
        return;
    }

    let request_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    let token = x_token.clone();
    let mut client = GeyserClient::with_interceptor(channel, move |mut req: Request<()>| {
        if let Some(ref t) = token {
            let val = MetadataValue::try_from(t.as_str())
                .map_err(|_| Status::internal("Invalid x-token value"))?;
            req.metadata_mut().insert("x-token", val);
        }
        Ok(req)
    })
    .max_decoding_message_size(64 * 1024 * 1024);

    let mut stream = match client.subscribe(request_stream).await {
        Ok(response) => response.into_inner(),
        Err(e) => {
            eprintln!("{} gRPC subscribe failed: {e:?}", "[ERROR]".red().bold());
            return;
        }
    };

    while let Some(message) = stream.next().await {
        match message {
            Ok(update) => match update.update_oneof {
                Some(UpdateOneof::Transaction(tx_update)) => {
                    metrics.tx_received.fetch_add(1, Ordering::Relaxed);
                    if let Some(acct) = extract_fee_payer(&tx_update) {
                        let _ = acct_queue.try_send(acct);
                    }
                    if let Some(parsed) = parse_transaction(&tx_update) {
                        if tx_queue.send(parsed).is_err() {
                            eprintln!("{} Worker queue closed", "[ERROR]".red().bold());
                            return;
                        }
                    }
                }
                Some(UpdateOneof::Slot(slot)) => {
                    metrics.slots.fetch_add(1, Ordering::Relaxed);
                    let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");
                    let parent = slot
                        .parent
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "—".into());
                    println!(
                        "{} {} - Slot: {} (Parent: {})",
                        "[SLOT]".cyan().bold(),
                        ts.to_string().dimmed(),
                        slot.slot.to_string().white().bold(),
                        parent.dimmed(),
                    );
                }
                _ => {}
            },
            Err(e) => eprintln!("{} Stream: {e:?}", "[ERROR]".red().bold()),
        }
    }
}

fn extract_fee_payer(tx_update: &SubscribeUpdateTransaction) -> Option<Account> {
    let tx_info = tx_update.transaction.as_ref()?;
    let msg = tx_info.transaction.as_ref()?.message.as_ref()?;
    let meta = tx_info.meta.as_ref()?;

    let pubkey = bs58::encode(msg.account_keys.first()?).into_string();
    let lamports = *meta.post_balances.first()? as i64;

    Some(Account {
        pubkey,
        lamports,
        slot: tx_update.slot as i64,
        executable: false,
        rent_epoch: 0,
    })
}

fn parse_transaction(tx_update: &SubscribeUpdateTransaction) -> Option<Transaction> {
    let tx_info = tx_update.transaction.as_ref()?;
    let signature = bs58::encode(&tx_info.signature).into_string();
    let slot = tx_update.slot as i64;

    let failed = tx_info
        .meta
        .as_ref()
        .map(|m| m.err.is_some())
        .unwrap_or(false);

    let amount = tx_info
        .meta
        .as_ref()
        .map(|meta| {
            meta.pre_balances
                .iter()
                .zip(meta.post_balances.iter())
                .map(|(&pre, &post)| pre.saturating_sub(post))
                .max()
                .unwrap_or(0) as f64
                / 1_000_000_000.0
        })
        .unwrap_or(0.0);

    let memo = extract_memo(tx_info);
    let (from, to) = extract_transfer_parties(tx_info);

    Some(Transaction {
        signature,
        slot,
        amount,
        failed,
        memo,
        from,
        to,
    })
}

fn extract_transfer_parties(
    tx_info: &SubscribeUpdateTransactionInfo,
) -> (Option<String>, Option<String>) {
    let (tx, meta) = match (tx_info.transaction.as_ref(), tx_info.meta.as_ref()) {
        (Some(t), Some(m)) => (t, m),
        _ => return (None, None),
    };
    let msg = match tx.message.as_ref() {
        Some(m) => m,
        None => return (None, None),
    };
    let keys = &msg.account_keys;

    let mut max_decrease: i64 = 0;
    let mut sender_idx: Option<usize> = None;
    let mut max_increase: i64 = 0;
    let mut receiver_idx: Option<usize> = None;

    for (i, (&pre, &post)) in meta
        .pre_balances
        .iter()
        .zip(meta.post_balances.iter())
        .enumerate()
    {
        let delta = pre as i64 - post as i64;
        if delta > max_decrease {
            max_decrease = delta;
            sender_idx = Some(i);
        }
        if -delta > max_increase {
            max_increase = -delta;
            receiver_idx = Some(i);
        }
    }

    let addr = |idx: Option<usize>| {
        idx.and_then(|i| keys.get(i))
            .map(|k| truncate_addr(&bs58::encode(k).into_string()))
    };

    (addr(sender_idx), addr(receiver_idx))
}

fn extract_memo(tx_info: &SubscribeUpdateTransactionInfo) -> Option<String> {
    let tx = tx_info.transaction.as_ref()?;
    let msg = tx.message.as_ref()?;

    let memo_idx = msg.account_keys.iter().position(|k| {
        let id = bs58::encode(k).into_string();
        id == MEMO_V1 || id == MEMO_V2
    })?;

    for ix in &msg.instructions {
        if ix.program_id_index as usize == memo_idx {
            if let Ok(s) = std::str::from_utf8(&ix.data) {
                let clean: String = s.chars().filter(|&c| c != '\0').collect();
                if !clean.is_empty() {
                    return Some(clean);
                }
            }
        }
    }

    if let Some(meta) = tx_info.meta.as_ref() {
        for inner in &meta.inner_instructions {
            for ix in &inner.instructions {
                if ix.program_id_index as usize == memo_idx {
                    if let Ok(s) = std::str::from_utf8(&ix.data) {
                        let clean: String = s.chars().filter(|&c| c != '\0').collect();
                        if !clean.is_empty() {
                            return Some(clean);
                        }
                    }
                }
            }
        }
    }

    None
}

pub(crate) fn truncate_addr(addr: &str) -> String {
    if addr.len() <= 16 {
        return addr.to_string();
    }
    format!("{}...{}", &addr[..8], &addr[addr.len() - 8..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_long_address() {
        let addr = "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU";
        let result = truncate_addr(addr);
        assert_eq!(&result[..8], &addr[..8]);
        assert_eq!(&result[result.len() - 8..], &addr[addr.len() - 8..]);
        assert!(result.contains("..."));
    }

    #[test]
    fn keeps_short_address() {
        let short = "ABC123";
        assert_eq!(truncate_addr(short), "ABC123");
    }
}
