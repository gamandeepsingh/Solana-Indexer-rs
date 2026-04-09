CREATE TABLE IF NOT EXISTS staging_transactions (
    signature VARCHAR(88),
    slot      BIGINT,
    success   BOOLEAN
);

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_transactions_slot
    ON transactions(slot);

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_transactions_created_at
    ON transactions(created_at DESC);

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_transactions_success
    ON transactions(success, created_at DESC);

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_large_transfers_amount
    ON large_transfers(amount DESC);

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_large_transfers_created_at
    ON large_transfers(created_at DESC);

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_memos_created_at
    ON memos(created_at DESC);

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_failed_tx_created_at
    ON failed_transactions(created_at DESC);
