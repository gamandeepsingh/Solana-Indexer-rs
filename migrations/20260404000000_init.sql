CREATE TABLE IF NOT EXISTS transactions (
    signature VARCHAR(88) PRIMARY KEY,
    slot      BIGINT NOT NULL,
    success   BOOLEAN NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS large_transfers (
    id        SERIAL PRIMARY KEY,
    signature VARCHAR(88) NOT NULL,
    slot      BIGINT NOT NULL,
    amount    DOUBLE PRECISION NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS memos (
    id        SERIAL PRIMARY KEY,
    signature VARCHAR(88) NOT NULL,
    memo      TEXT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS accounts (
    id         SERIAL PRIMARY KEY,
    pubkey     VARCHAR(88) NOT NULL UNIQUE,
    lamports   BIGINT NOT NULL,
    slot       BIGINT NOT NULL,
    executable BOOLEAN NOT NULL,
    rent_epoch BIGINT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS failed_transactions (
    id        SERIAL PRIMARY KEY,
    signature VARCHAR(88) NOT NULL,
    slot      BIGINT NOT NULL,
    error     TEXT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);
