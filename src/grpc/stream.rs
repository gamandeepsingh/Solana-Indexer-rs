use std::collections::HashMap;

use chrono::Utc;
use colored::Colorize;
use sqlx::PgPool;
use tokio::sync::mpsc::{self, Sender};
use tokio_stream::StreamExt;
use tonic::{Request, Status, metadata::MetadataValue, transport::Channel};
use yellowstone_grpc_proto::geyser::{
    CommitmentLevel, SubscribeRequest, SubscribeRequestFilterSlots,
    SubscribeRequestFilterTransactions, SubscribeUpdateTransaction, SubscribeUpdateTransactionInfo,
    geyser_client::GeyserClient, subscribe_update::UpdateOneof,
};

use crate::db::queries::upsert_account;
use crate::models::account::Account;
use crate::models::transaction::Transaction;

const MEMO_V1: &str = "Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo";
const MEMO_V2: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";

pub async fn start_stream(
    channel: Channel,
    tx_queue: Sender<Transaction>,
    db: PgPool,
    x_token: Option<String>,
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

    let mut client = GeyserClient::with_interceptor(channel, move |mut req: Request<()>| {
        if let Some(ref token) = x_token {
            let val = MetadataValue::try_from(token.as_str())
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
                    if let Some(acct) = extract_fee_payer(&tx_update) {
                        let db_clone = db.clone();
                        tokio::spawn(async move {
                            if let Err(e) = upsert_account(&db_clone, &acct).await {
                                eprintln!("{} upsert_account: {e}", "[ERROR]".red().bold());
                            }
                        });
                    }
                    if let Some(parsed) = parse_transaction(&tx_update) {
                        if tx_queue.send(parsed).await.is_err() {
                            eprintln!("{} Worker queue closed", "[ERROR]".red().bold());
                            return;
                        }
                    }
                }
                Some(UpdateOneof::Slot(slot)) => {
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

    eprintln!("{} gRPC stream ended", "[WARN]".yellow().bold());
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
                return Some(s.to_string());
            }
        }
    }

    if let Some(meta) = tx_info.meta.as_ref() {
        for inner in &meta.inner_instructions {
            for ix in &inner.instructions {
                if ix.program_id_index as usize == memo_idx {
                    if let Ok(s) = std::str::from_utf8(&ix.data) {
                        return Some(s.to_string());
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
