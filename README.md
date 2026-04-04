# Solana Real-time Transaction Indexer

[![Rust](https://img.shields.io/badge/Rust-2024-orange?logo=rust)](https://www.rust-lang.org/)
[![Solana](https://img.shields.io/badge/Solana-mainnet-9945FF?logo=solana)](https://solana.com/)
[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-16-316192?logo=postgresql)](https://www.postgresql.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A high-performance Solana blockchain indexer written in Rust. Streams live data via **Yellowstone gRPC**, filters it, and stores it in **PostgreSQL** in real-time.

---

## Architecture


- **Transaction queue** (bounded 1000): decouples the stream from DB writes, provides backpressure
- **Account upserts**: spawned directly — keeps the unique account registry up to date
- **Slot updates**: printed to console only (network health indicator)

---

## Live Output

```
→ Connecting to database...
✓ Database connected
→ Connecting to https://solana-rpc.parafi.tech:10443
✓ gRPC connected

  Streaming live Solana transactions...
────────────────────────────────────────────────────────────
[SLOT] 2026-04-04T17:26:32.033Z - Slot: 411016786 (Parent: 411016785)
[TX]   2026-04-04T17:26:32.695Z - 3vpDTvHg...qVwTY28  0.0000 SOL
[TX]   2026-04-04T17:26:33.135Z - 5dgwcf1X...iJU25    0.0100 SOL

[TRANSFER] 2026-04-04T17:26:34.105Z
  From: 7xKXtg2C...QoQ9HD8i
  To:   9WzDXwBb...i4qRvKn3
  Amount: 5.2500 SOL

[MEMO] 2026-04-04T17:26:35.412Z - Payment ID: TXN_12345
```

| Prefix | Color | Meaning |
|---|---|---|
| `[SLOT]` | Cyan | New confirmed slot |
| `[TX]` white | White | Successful transaction |
| `[TX]` red | Red | Failed transaction |
| `[TRANSFER]` | Yellow | Large SOL transfer (> 1 SOL) |
| `[MEMO]` | Magenta | Transaction with memo payload |
| `[ERROR]` | Red | Runtime error |

---

## Database Schema

All 5 tables are populated by the indexer:

| Table | Populated by | Description |
|---|---|---|
| `transactions` | every non-failed tx | Core record: signature, slot, success |
| `failed_transactions` | every failed tx | Error tracking |
| `large_transfers` | tx with SOL move > 1 SOL(configurable) | Whale / large transfer monitoring |
| `memos` | tx using Memo v1 or v2 program | Payment references |
| `accounts` | every account update touching a tx | Live account state registry |

---

## Quick Start

### Prerequisites

- Rust 1.83+
- PostgreSQL 14+ (or use Docker Compose)
- A Yellowstone gRPC endpoint (Helius, Triton, Parafi, etc.)

### 1. Clone and configure

```bash
git clone <repo-url>
cd solana-indexer
cp .env.example .env
```

Edit `.env`:

```env
GRPC_ENDPOINT=https://your-yellowstone-endpoint
DATABASE_URL=postgresql://user:password@localhost:5432/solana_indexer
X_TOKEN=your-api-key-if-required
CONSOLE_LOG=false
```

### 2. Run the database migration

```bash
psql $DATABASE_URL -f migrations/20260404000000_init.sql
```

### 3. Run

```bash
cargo run --release
```

---

## Docker

```bash
# Set GRPC_ENDPOINT and X_TOKEN in .env, then:
docker-compose up -d

# Follow logs
docker-compose logs -f indexer
```

`docker-compose up` spins up PostgreSQL and runs the indexer. The migration is applied automatically on first start.

---

## Configuration

| Variable | Required | Description |
|---|---|---|
| `GRPC_ENDPOINT` | Yes | Yellowstone gRPC URL (e.g. `https://...`) |
| `DATABASE_URL` | Yes | PostgreSQL connection string |
| `X_TOKEN` | If required | API key for authenticated endpoints (Helius, etc.) |
| `CONSOLE_LOG` | No | Log level for internal libs (default: `false`) |

---

## Running Tests

```bash
cargo test
```

Unit tests cover:
- `processor::filters` — `is_large_transfer` threshold logic
- `processor::transaction` — signature truncation
- `grpc::stream` — address truncation

---

## Project Structure

```
src/
├── main.rs              # Entry point
├── config.rs            # Env var loading
├── grpc/
│   ├── client.rs        # TLS channel setup
│   └── stream.rs        # Subscribe loop, parsing, account handler
├── db/
│   ├── connection.rs    # PgPool setup
│   └── queries.rs       # All SQL insert/upsert functions
├── models/
│   ├── transaction.rs   # Transaction struct
│   └── account.rs       # Account struct
├── processor/
│   ├── filters.rs       # is_large_transfer threshold
│   └── transaction.rs   # handle(): routes tx to correct DB table + console
└── workers/
    └── queue.rs         # Bounded mpsc channel + worker task
migrations/
└── 20260404000000_init.sql   # All 5 tables
Dockerfile
docker-compose.yml
```

---

## Tech Stack

- **[Tokio](https://tokio.rs/)** — async runtime
- **[Tonic](https://github.com/hyperium/tonic)** — gRPC client (HTTP/2 + TLS)
- **[yellowstone-grpc-proto](https://crates.io/crates/yellowstone-grpc-proto)** — Geyser proto types
- **[SQLx](https://github.com/launchbadge/sqlx)** — async PostgreSQL driver
- **[colored](https://crates.io/crates/colored)** — terminal colors
- **[chrono](https://crates.io/crates/chrono)** — timestamps
- **[bs58](https://crates.io/crates/bs58)** — base58 encoding for Solana pubkeys

---

**Built by [Gamandeep](https://x.com/gamandeepsingh4) for the Solana ecosystem**
