# Solana Real-time Transaction Indexer

[![Rust](https://img.shields.io/badge/Rust-2024-orange?logo=rust)](https://www.rust-lang.org/)
[![Solana](https://img.shields.io/badge/Solana-mainnet-9945FF?logo=solana)](https://solana.com/)
[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-16-316192?logo=postgresql)](https://www.postgresql.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A high-performance Solana blockchain indexer written in Rust. Streams live data via **Yellowstone gRPC**, filters it, and stores it in **PostgreSQL** in real-time with ~99.5% efficiency at mainnet throughput.

Blog: [link](https://medium.com/@gamandeepsingh4/building-a-real-time-solana-indexer-in-rust-a-complete-guide-edcf64119691)

---

## Architecture

<img width="1536" height="1024" alt="architect" src="https://github.com/user-attachments/assets/c5c9bb1c-cf97-4c7d-b8d6-cc1ccb43665a" />

```
Yellowstone gRPC (Solana validator stream)
        │
        ▼
  grpc/stream.rs          ← TLS channel, subscribe, parse
        │
        ├──► [SLOT update] → print to terminal
        │
        └──► [TX update]
                │
                ├── fee payer → acct_queue (unbounded, non-blocking)
                │                    │
                │               account worker
                │               batch 200 / 2s → accounts table
                │
                └── parse_transaction()
                        │
                        ▼
              tx_queue (unbounded — never blocks, never drops)
                        │
                        ▼
              workers/queue.rs (batch worker)
              collect 200 txns or 100ms timer
                        │
               tokio::join! (4 parallel writes)
               ┌─────────┬──────────┬────────┬──────────────┐
               ▼         ▼          ▼        ▼
          COPY into  batch INSERT  batch    batch INSERT
          transactions failed_txns memos  large_transfers
          (via staging table)
```

### Key design decisions

| Decision | Detail |
|---|---|
| **Unbounded tx channel** | `send()` never blocks the gRPC read loop — eliminates "lagged to send an update" disconnects |
| **COPY protocol** | `COPY FROM STDIN` + staging table merge is 5–10× faster than `INSERT VALUES` for the high-volume transactions table |
| **Batch size 200 / 100ms** | Reduces DB round-trips ~200× vs one-insert-per-tx while keeping max latency under 200ms |
| **Account batching** | Separate worker with 2s flush interval — replaces the old per-tx `tokio::spawn` that created 4000+ tasks/sec |
| **Auto-reconnect** | Exponential backoff (4s → 8s → 16s → 30s cap), resets after 60s stable session |
| **Separate write/read pools** | 20 connections each — write pool for workers, read pool reserved for future API layer |

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

---

## Project Structure

```
src/
├── main.rs              # Entry point, graceful shutdown
├── config.rs            # Env var config
├── metrics.rs           # AtomicU64 counters + file reporter
├── grpc/
│   ├── client.rs        # TLS channel, HTTP/2 tuning
│   └── stream.rs        # Subscribe loop, parse, auto-reconnect
├── db/
│   ├── connection.rs    # Separate write/read PgPool (20 conn each)
│   └── queries.rs       # COPY + batch INSERT/UPSERT functions
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

Covers:
- `processor::filters` — large transfer threshold
- `processor::transaction` — signature truncation
- `grpc::stream` — address truncation

---

## Tech Stack

- **[Tokio](https://tokio.rs/)** — async runtime
- **[Tonic](https://github.com/hyperium/tonic)** — gRPC client (HTTP/2 + TLS)
- **[yellowstone-grpc-proto](https://crates.io/crates/yellowstone-grpc-proto)** — Geyser proto types
- **[SQLx](https://github.com/launchbadge/sqlx)** — async PostgreSQL driver with COPY support
- **[colored](https://crates.io/crates/colored)** — terminal colors
- **[chrono](https://crates.io/crates/chrono)** — timestamps
- **[bs58](https://crates.io/crates/bs58)** — base58 encoding for Solana pubkeys

---

**Built by [Gamandeep](https://x.com/gamandeepsingh4) for the Solana ecosystem**
