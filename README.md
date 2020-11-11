# <h1 align="center"> MEV Inspect </h1>

**Ethereum MEV Inspector in Rust**

## Installing

`cargo build --release`

## Running the CLI

```
Usage: ./target/debug/mev-inspect [OPTIONS]

Optional arguments:
  -h, --help
  -u, --url URL          The tracing / archival node's URL (default: http://localhost:8545)
  -c, --cache CACHE      Path to where traces will be cached (default: res)
  -d, --db-url DB-URL    the database's url (default: localhost)
  -D, --db-user DB-USER  the user of the database (default: postgres)
  --db-table DB-TABLE    the table of the database (default: mev_inspections)

Available commands:
  tx      inspect a transaction
  blocks  inspect a range of blocks
```

## Running the tests

**Tests require `postgres` installed.**

`cargo test`
