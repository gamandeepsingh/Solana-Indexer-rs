use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool, QueryBuilder};

use crate::models::account::Account;
use crate::models::transaction::Transaction;

#[derive(Debug, FromRow, Serialize)]
pub struct TxRow {
    pub signature: String,
    pub slot: i64,
    pub success: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow, Serialize)]
pub struct TransferRow {
    pub id: i32,
    pub signature: String,
    pub slot: i64,
    pub amount: f64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow, Serialize)]
pub struct MemoRow {
    pub id: i32,
    pub signature: String,
    pub memo: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow, Serialize)]
pub struct AccountRow {
    pub pubkey: String,
    pub lamports: i64,
    pub slot: i64,
    pub executable: bool,
    pub rent_epoch: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct StatsRow {
    pub total_transactions: i64,
    pub total_failed: i64,
    pub total_transfers: i64,
    pub total_memos: i64,
    pub total_accounts: i64,
}

pub async fn read_transactions(
    pool: &PgPool,
    limit: i64,
    offset: i64,
    success: Option<bool>,
) -> Result<(Vec<TxRow>, i64), sqlx::Error> {
    let (rows, total) = match success {
        Some(s) => {
            let rows = sqlx::query_as::<_, TxRow>(
                "SELECT signature, slot, success, created_at FROM transactions
                 WHERE success = $1
                 ORDER BY created_at DESC
                 LIMIT $2 OFFSET $3",
            )
            .bind(s)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await?;
            let total: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM transactions WHERE success = $1")
                    .bind(s)
                    .fetch_one(pool)
                    .await?;
            (rows, total)
        }
        None => {
            let rows = sqlx::query_as::<_, TxRow>(
                "SELECT signature, slot, success, created_at FROM transactions
                 ORDER BY created_at DESC
                 LIMIT $1 OFFSET $2",
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await?;
            let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transactions")
                .fetch_one(pool)
                .await?;
            (rows, total)
        }
    };
    Ok((rows, total))
}

pub async fn read_transaction(
    pool: &PgPool,
    signature: &str,
) -> Result<Option<TxRow>, sqlx::Error> {
    sqlx::query_as::<_, TxRow>(
        "SELECT signature, slot, success, created_at FROM transactions WHERE signature = $1",
    )
    .bind(signature)
    .fetch_optional(pool)
    .await
}

pub async fn read_slot_transactions(
    pool: &PgPool,
    slot: i64,
) -> Result<Vec<TxRow>, sqlx::Error> {
    sqlx::query_as::<_, TxRow>(
        "SELECT signature, slot, success, created_at FROM transactions
         WHERE slot = $1
         ORDER BY created_at ASC",
    )
    .bind(slot)
    .fetch_all(pool)
    .await
}

pub async fn read_transfers(
    pool: &PgPool,
    limit: i64,
    offset: i64,
    min_amount: f64,
) -> Result<(Vec<TransferRow>, i64), sqlx::Error> {
    let rows = sqlx::query_as::<_, TransferRow>(
        "SELECT id, signature, slot, amount, created_at FROM large_transfers
         WHERE amount >= $1
         ORDER BY amount DESC
         LIMIT $2 OFFSET $3",
    )
    .bind(min_amount)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    let total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM large_transfers WHERE amount >= $1")
            .bind(min_amount)
            .fetch_one(pool)
            .await?;

    Ok((rows, total))
}

pub async fn read_memos(
    pool: &PgPool,
    limit: i64,
    offset: i64,
) -> Result<(Vec<MemoRow>, i64), sqlx::Error> {
    let rows = sqlx::query_as::<_, MemoRow>(
        "SELECT id, signature, memo, created_at FROM memos
         ORDER BY created_at DESC
         LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memos")
        .fetch_one(pool)
        .await?;

    Ok((rows, total))
}

pub async fn read_account(
    pool: &PgPool,
    pubkey: &str,
) -> Result<Option<AccountRow>, sqlx::Error> {
    sqlx::query_as::<_, AccountRow>(
        "SELECT pubkey, lamports, slot, executable, rent_epoch, created_at
         FROM accounts WHERE pubkey = $1",
    )
    .bind(pubkey)
    .fetch_optional(pool)
    .await
}

pub async fn read_stats(pool: &PgPool) -> Result<StatsRow, sqlx::Error> {
    let q = |table: &'static str| async move {
        sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(n_live_tup, 0)
             FROM pg_stat_user_tables
             WHERE relname = $1",
        )
        .bind(table)
        .fetch_optional(pool)
        .await
        .map(|opt| opt.unwrap_or(0))
    };

    let (total_transactions, total_failed, total_transfers, total_memos, total_accounts) =
        tokio::try_join!(
            q("transactions"),
            q("failed_transactions"),
            q("large_transfers"),
            q("memos"),
            q("accounts"),
        )?;

    Ok(StatsRow {
        total_transactions,
        total_failed,
        total_transfers,
        total_memos,
        total_accounts,
    })
}

pub async fn copy_transactions(pool: &PgPool, txs: &[Transaction]) -> Result<(), sqlx::Error> {
    let items: Vec<&Transaction> = txs.iter().filter(|t| !t.failed).collect();
    if items.is_empty() {
        return Ok(());
    }

    let mut tsv = String::with_capacity(items.len() * 110);
    for tx in &items {
        tsv.push_str(&tx.signature);
        tsv.push('\t');
        tsv.push_str(&tx.slot.to_string());
        tsv.push_str("\tt\n");
    }

    let mut conn = pool.acquire().await?;

    let mut copy = conn
        .copy_in_raw(
            "COPY staging_transactions (signature, slot, success) FROM STDIN (FORMAT TEXT)",
        )
        .await?;
    copy.send(tsv.as_bytes()).await?;
    copy.finish().await?;

    sqlx::query(
        "INSERT INTO transactions (signature, slot, success)
         SELECT signature, slot, success FROM staging_transactions
         ON CONFLICT (signature) DO NOTHING",
    )
    .execute(pool)
    .await?;

    sqlx::query("TRUNCATE staging_transactions")
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn batch_insert_failed(pool: &PgPool, txs: &[Transaction]) -> Result<(), sqlx::Error> {
    let items: Vec<&Transaction> = txs.iter().filter(|t| t.failed).collect();
    if items.is_empty() {
        return Ok(());
    }
    let mut qb = QueryBuilder::new("INSERT INTO failed_transactions (signature, slot, error) ");
    qb.push_values(&items, |mut b, tx| {
        b.push_bind(&tx.signature)
            .push_bind(tx.slot)
            .push_bind("Transaction failed");
    });
    qb.build().execute(pool).await?;
    Ok(())
}

pub async fn batch_insert_memos(pool: &PgPool, txs: &[Transaction]) -> Result<(), sqlx::Error> {
    let items: Vec<(&str, &str)> = txs
        .iter()
        .filter_map(|t| t.memo.as_deref().map(|m| (t.signature.as_str(), m)))
        .collect();
    if items.is_empty() {
        return Ok(());
    }
    let mut qb = QueryBuilder::new("INSERT INTO memos (signature, memo) ");
    qb.push_values(&items, |mut b, (sig, memo)| {
        b.push_bind(*sig).push_bind(*memo);
    });
    qb.build().execute(pool).await?;
    Ok(())
}

pub async fn batch_insert_transfers(
    pool: &PgPool,
    txs: &[Transaction],
    threshold: f64,
) -> Result<(), sqlx::Error> {
    let items: Vec<&Transaction> = txs
        .iter()
        .filter(|t| !t.failed && t.amount > threshold)
        .collect();
    if items.is_empty() {
        return Ok(());
    }
    let mut qb = QueryBuilder::new("INSERT INTO large_transfers (signature, slot, amount) ");
    qb.push_values(&items, |mut b, tx| {
        b.push_bind(&tx.signature)
            .push_bind(tx.slot)
            .push_bind(tx.amount);
    });
    qb.build().execute(pool).await?;
    Ok(())
}

pub async fn batch_upsert_accounts(pool: &PgPool, accounts: &[Account]) -> Result<(), sqlx::Error> {
    if accounts.is_empty() {
        return Ok(());
    }

    let mut map: std::collections::HashMap<&str, &Account> =
        std::collections::HashMap::with_capacity(accounts.len());
    for a in accounts {
        map.entry(a.pubkey.as_str())
            .and_modify(|prev| {
                if a.slot > prev.slot {
                    *prev = a;
                }
            })
            .or_insert(a);
    }
    let deduped: Vec<&Account> = map.into_values().collect();

    let mut qb = QueryBuilder::new(
        "INSERT INTO accounts (pubkey, lamports, slot, executable, rent_epoch) ",
    );
    qb.push_values(&deduped, |mut b, a| {
        b.push_bind(&a.pubkey)
            .push_bind(a.lamports)
            .push_bind(a.slot)
            .push_bind(a.executable)
            .push_bind(a.rent_epoch);
    });
    qb.push(
        " ON CONFLICT (pubkey) DO UPDATE SET
             lamports   = EXCLUDED.lamports,
             slot       = EXCLUDED.slot,
             executable = EXCLUDED.executable,
             rent_epoch = EXCLUDED.rent_epoch",
    );
    qb.build().execute(pool).await?;
    Ok(())
}
