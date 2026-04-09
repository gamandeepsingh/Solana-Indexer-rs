# Solana Real-time Transaction Indexer

[![Rust](https://img.shields.io/badge/Rust-2024-orange?logo=rust)](https://www.rust-lang.org/)
[![Solana](https://img.shields.io/badge/Solana-mainnet-9945FF?logo=solana)](https://solana.com/)
[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-16-316192?logo=postgresql)](https://www.postgresql.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A high-performance Solana blockchain indexer written in Rust. Streams live data via **Yellowstone gRPC**, filters it, and stores it in **PostgreSQL** in real-time with ~99.5% efficiency at mainnet throughput.

Blog: [link](https://medium.com/@gamandeepsingh4/building-a-real-time-solana-indexer-in-rust-a-complete-guide-edcf64119691)

---

## Architecture
<img width="1199" height="1312" alt="arch1" src="https://github.com/user-attachments/assets/b0a9f41e-41db-4341-9a90-44b3b2d61718" />


### Key design decisions

| Decision | Detail |
|---|---|
| **Unbounded tx channel** | `send()` never blocks the gRPC read loop — eliminates "lagged to send an update" disconnects |
| **COPY protocol** | `COPY FROM STDIN` + staging table merge is 5–10× faster than `INSERT VALUES` for the high-volume transactions table |
| **Batch size 200 / 100ms** | Reduces DB round-trips ~200× vs one-insert-per-tx while keeping max latency under 200ms |
| **Account batching** | Separate worker with 2s flush interval — replaces the old per-tx `tokio::spawn` that created 4000+ tasks/sec |
| **Auto-reconnect** | Exponential backoff (4s → 8s → 16s → 30s cap), resets after 60s stable session |
| **Separate write/read pools** | 20 connections each — write pool for indexer workers, read pool exclusively for the API layer |

---

## REST API

The indexer exposes a read-only HTTP API on port `3000` (configurable via `API_PORT`). It runs on the same Tokio runtime as the indexer — no separate process needed.

### Endpoints

| Method | Path | Query params | Description |
|---|---|---|---|
| `GET` | `/health` | — | Liveness check. No DB call. |
| `GET` | `/api/stats` | — | Approximate row counts for all tables. |
| `GET` | `/api/transactions` | `limit`, `offset`, `success` | Paginated transaction list, newest first. |
| `GET` | `/api/transactions/:signature` | — | Single transaction by signature. 404 if not found. |
| `GET` | `/api/slots/:slot` | — | All transactions in a given slot. |
| `GET` | `/api/transfers` | `limit`, `offset`, `min_amount` | Large transfers, ordered by amount descending. |
| `GET` | `/api/memos` | `limit`, `offset` | Memo transactions, newest first. |
| `GET` | `/api/accounts/:pubkey` | — | Account info by public key. 404 if not found. |

All paginated endpoints return:

```json
{
  "data": [...],
  "total": 1000000,
  "limit": 50,
  "offset": 0
}
```

### Examples

```bash
# Liveness
curl http://localhost:3000/health
# {"status":"ok","uptime_secs":142}

# Stats (instant — no COUNT(*))
curl http://localhost:3000/api/stats
# {"total_transactions":4821003,"total_failed":1203442,...}

# 10 most recent transactions
curl "http://localhost:3000/api/transactions?limit=10"

# Only failed transactions, page 2
curl "http://localhost:3000/api/transactions?success=false&limit=50&offset=50"

# Single transaction
curl http://localhost:3000/api/transactions/3vpDTvHg...qVwTY28

# All transactions in a slot
curl http://localhost:3000/api/slots/411483492

# Transfers above 10 SOL
curl "http://localhost:3000/api/transfers?min_amount=10"

# Account info
curl http://localhost:3000/api/accounts/7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU
```

### Why the API is fast

Every endpoint is designed to avoid slow paths entirely.

**Dedicated read pool** — The API uses its own 20-connection PostgreSQL pool, completely separate from the write workers. A write-heavy ingest burst never starves a read query of a connection.

**Pre-built indexes on every query column** — All filter and sort columns are indexed. No sequential scans at any table size:

| Endpoint | Index hit |
|---|---|
| `GET /api/transactions?success=…` | `idx_transactions_success (success, created_at DESC)` |
| `GET /api/transactions` | `idx_transactions_created_at (created_at DESC)` |
| `GET /api/slots/:slot` | `idx_transactions_slot (slot)` |
| `GET /api/transfers?min_amount=…` | `idx_large_transfers_amount (amount DESC)` |
| `GET /api/memos` | `idx_memos_created_at (created_at DESC)` |
| `GET /api/transactions/:signature` | Primary key lookup |
| `GET /api/accounts/:pubkey` | Unique index on `pubkey` |

**O(1) stats via `pg_stat_user_tables`** — `/api/stats` reads `n_live_tup` from PostgreSQL's internal statistics table rather than running `COUNT(*)`. This is a single row lookup regardless of how many rows exist, making stats ~100× faster than a full table scan on a large DB.

**Zero serialization overhead** — Response types derive both `sqlx::FromRow` and `serde::Serialize`, so rows are decoded from the wire and serialized to JSON in one pass with no intermediate allocation.

**Axum on Tokio** — The HTTP server shares the same async runtime as the indexer. No thread-per-request overhead, no context switches between runtimes.

---

## Live Output

```
→ Connecting to database...
✓ Database connected
→ Connecting to https://solana-rpc.parafi.tech:10443
✓ gRPC connected

  Streaming live Solana transactions...
→ Benchmark logging → benchmark.log
────────────────────────────────────────────────────────────
[SLOT] 2026-04-06T20:23:23.043Z - Slot: 411483492 (Parent: 411483491)
[TX]   2026-04-06T20:23:23.412Z - 3vpDTvHg...qVwTY28  0.0000 SOL
[TRANSFER] 2026-04-06T20:23:24.105Z
  From: 7xKXtg2C...QoQ9HD8i
  To:   9WzDXwBb...i4qRvKn3
  Amount: 5.2500 SOL
[MEMO] 2026-04-06T20:23:25.412Z - Payment ID: TXN_12345
```

<img width="513" height="317" alt="Screenshot 2026-04-05 at 1 30 32 AM" src="https://github.com/user-attachments/assets/7925892e-1b60-45dd-b967-907886129fda" />

| Prefix | Color | Meaning |
|---|---|---|
| `[SLOT]` | Cyan | New confirmed slot (~150/min on mainnet) |
| `[TX]` white | White | Successful transaction |
| `[TX]` red | Red | Failed transaction |
| `[TRANSFER]` | Yellow | SOL transfer above threshold (default 1 SOL) |
| `[MEMO]` | Magenta | Transaction containing Memo program data |

---

## Benchmark Log

Throughput metrics are written to `benchmark.log` (configurable via `BENCH_LOG`) every 5 minutes — not the terminal.

```
--- session started 2026-04-06T19:54:10Z ---
timestamp            | tps   | recv      | written   | failed    | failed% | memos | xfers | accts     | batches | slots
----------------------------------------------------------------------------------------------------------
2026-04-06T19:54:15Z |   319 |      1635 |       968 |       631 |     39% |     7 |   121 |         0 |      30 |     8
2026-04-06T19:54:20Z |   305 |      3170 |      2175 |       952 |     30% |    20 |   228 |         0 |      70 |    16
2026-04-06T19:54:25Z |   507 |      5677 |      4252 |      1414 |     24% |    38 |   470 |         1 |     119 |    31
```

| Column | Meaning |
|---|---|
| `tps` | Transactions processed per second in this interval |
| `recv` | Total received from gRPC stream (cumulative) |
| `written` | Successfully inserted into `transactions` table |
| `failed` | Inserted into `failed_transactions` |
| `failed%` | Chain-level failure rate (normal: 20–40% on mainnet) |
| `memos` | Memo program transactions captured |
| `xfers` | Large transfers captured |
| `accts` | Account rows upserted |
| `batches` | Total flush cycles |

---

## Database Schema

### Tables

| Table | Insert method | Description |
|---|---|---|
| `transactions` | `COPY` + staging merge | Every confirmed non-vote transaction |
| `failed_transactions` | Batch `INSERT` | Transactions that failed on-chain |
| `large_transfers` | Batch `INSERT` | SOL movements above threshold |
| `memos` | Batch `INSERT` | Memo program payloads |
| `accounts` | Batch `UPSERT` | Fee payer accounts, latest balance |

### Indexes (migration `20260406000000_performance.sql`)

```sql
idx_transactions_slot
idx_transactions_created_at
idx_transactions_success
idx_large_transfers_amount
idx_large_transfers_created_at
idx_memos_created_at
idx_failed_tx_created_at
```

---

## Quick Start

### Prerequisites

- Rust 1.86+
- PostgreSQL 14+ (or Supabase)
- A Yellowstone gRPC endpoint (Helius, Triton, Parafi, etc.)

### 1. Clone and configure

```bash
git clone https://github.com/gamandeepsingh/solana-indexer
cd solana-indexer
cp .env.example .env
```

Edit `.env`:

```env
GRPC_ENDPOINT=https://your-yellowstone-endpoint
DATABASE_URL=postgresql://user:password@host:5432/dbname
X_TOKEN=your-api-key-if-required
CONSOLE_LOG=true
BENCH_LOG=benchmark.log
API_PORT=3000
```

### 2. Run migrations

```bash
psql $DATABASE_URL -f migrations/20260404000000_init.sql
psql $DATABASE_URL -f migrations/20260406000000_performance.sql
```

### 3. Run

```bash
cargo run --release
```

---

## Docker

```bash
docker-compose up -d
docker-compose logs -f indexer
```

---

## Configuration

| Variable | Required | Default | Description |
|---|---|---|---|
| `GRPC_ENDPOINT` | Yes | — | Yellowstone gRPC URL |
| `DATABASE_URL` | Yes | — | PostgreSQL connection string |
| `X_TOKEN` | If required | — | API key for authenticated endpoints |
| `CONSOLE_LOG` | No | `true` | Set `false` to silence terminal output |
| `BENCH_LOG` | No | `benchmark.log` | Path for benchmark metrics log file |
| `API_PORT` | No | `3000` | Port the REST API listens on |

---

## Project Structure

```
src/
├── main.rs              # Entry point, dual pool init, graceful shutdown
├── config.rs            # Env var config (incl. API_PORT)
├── metrics.rs           # AtomicU64 counters + file reporter
├── api/
│   ├── mod.rs           # Axum router, CORS, serve()
│   └── handlers.rs      # HTTP handlers + response types + tests
├── grpc/
│   ├── client.rs        # TLS channel, HTTP/2 tuning
│   └── stream.rs        # Subscribe loop, parse, auto-reconnect
├── db/
│   ├── connection.rs    # Separate write/read PgPool (20 conn each)
│   └── queries.rs       # COPY + batch INSERT/UPSERT + read query fns
├── models/
│   ├── transaction.rs   # Transaction struct
│   └── account.rs       # Account struct
├── processor/
│   ├── filters.rs       # is_large_transfer threshold
│   └── transaction.rs   # print_tx() — terminal output only
└── workers/
    └── queue.rs         # Unbounded tx queue + batch worker + account worker
migrations/
├── 20260404000000_init.sql              # 5 tables
├── 20260405000000_drop_account_owner.sql
└── 20260406000000_performance.sql       # Staging table + 7 indexes
```

---

## Tests

```bash
cargo test
```

Covers 32 test cases across:

| Module | Tests |
|---|---|
| `processor::filters` | Large transfer threshold detection |
| `processor::transaction` | Signature truncation |
| `grpc::stream` | Address truncation |
| `api::handlers` | All 8 endpoints — status codes, response shape, pagination, filters, sort order, 404 behaviour |

API tests that require a database connection are automatically skipped when `DATABASE_URL` is not set, so `cargo test` always passes in CI without a database.

```bash
# Run with a live DB to exercise all 32 tests
DATABASE_URL=postgresql://... cargo test
```

---

## Tech Stack

- **[Tokio](https://tokio.rs/)** — async runtime
- **[Axum](https://github.com/tokio-rs/axum)** — HTTP API server (runs on the same Tokio runtime)
- **[Tonic](https://github.com/hyperium/tonic)** — gRPC client (HTTP/2 + TLS)
- **[yellowstone-grpc-proto](https://crates.io/crates/yellowstone-grpc-proto)** — Geyser proto types
- **[SQLx](https://github.com/launchbadge/sqlx)** — async PostgreSQL driver with COPY support
- **[tower-http](https://crates.io/crates/tower-http)** — CORS + request tracing middleware
- **[Serde](https://serde.rs/)** — zero-copy JSON serialization
- **[colored](https://crates.io/crates/colored)** — terminal colors
- **[chrono](https://crates.io/crates/chrono)** — timestamps
- **[bs58](https://crates.io/crates/bs58)** — base58 encoding for Solana pubkeys

---

**Built by [Gamandeep](https://x.com/gamandeepsingh4) for the Solana ecosystem**
