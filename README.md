# <h1 align="center"> MEV Inspect </h1>

**Ethereum MEV Inspector in Rust**

## Inspectors

- Curve
- Balancer
- Uniswap (& clones)
- Aave
- Compound

## Installing

`cargo build --release`

## Running the CLI

```
Usage: ./target/release/mev-inspect [OPTIONS]

Optional arguments:
  -h, --help
  -r, --reset              clear and re-build the database
  -o, --overwrite          do not skip blocks which already exist
  -u, --url URL            The tracing / archival node's URL (default: http://localhost:8545)
  -c, --cache CACHE        Path to where traces will be cached
  -d, --db-cfg DB-CFG      Database config
  -D, --db-table DB-TABLE  the table of the database (default: mev_inspections)

Available commands:
  tx      inspect a transaction
  blocks  inspect a range of blocks
```

## Running the tests

**Tests require `postgres` installed.**

`cargo test`
