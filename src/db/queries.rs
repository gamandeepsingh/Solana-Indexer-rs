use sqlx::{PgPool, QueryBuilder};

use crate::models::account::Account;
use crate::models::transaction::Transaction;

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
