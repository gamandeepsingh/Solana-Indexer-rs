use sqlx::{PgPool, QueryBuilder};

use crate::models::account::Account;
use crate::models::transaction::Transaction;

// ── Single inserts (kept for compatibility) ──────────────────────────────────

pub async fn upsert_account(pool: &PgPool, account: &Account) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO accounts (pubkey, lamports, slot, executable, rent_epoch)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (pubkey) DO UPDATE SET
             lamports   = EXCLUDED.lamports,
             slot       = EXCLUDED.slot,
             executable = EXCLUDED.executable,
             rent_epoch = EXCLUDED.rent_epoch",
    )
    .bind(&account.pubkey)
    .bind(account.lamports)
    .bind(account.slot)
    .bind(account.executable)
    .bind(account.rent_epoch)
    .execute(pool)
    .await?;
    Ok(())
}

// ── Batch inserts ─────────────────────────────────────────────────────────────

pub async fn batch_insert_transactions(
    pool: &PgPool,
    txs: &[Transaction],
) -> Result<(), sqlx::Error> {
    let items: Vec<&Transaction> = txs.iter().filter(|t| !t.failed).collect();
    if items.is_empty() {
        return Ok(());
    }
    let mut qb = QueryBuilder::new("INSERT INTO transactions (signature, slot, success) ");
    qb.push_values(&items, |mut b, tx| {
        b.push_bind(&tx.signature)
            .push_bind(tx.slot)
            .push_bind(true);
    });
    qb.push(" ON CONFLICT DO NOTHING");
    qb.build().execute(pool).await?;
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
