# <h1 align="center"> MEV Inspect </h1>

**Polygon MEV Inspector in Rust**

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
Example:
```
./target/release/mev-inspect --db-cfg postgresql://postgres:postgres@localhost -u http://IPADDR:8545 tx 0x5243f353cf41f8394ba480e3c15fb57881a5d8ec985874520a1b322ecf2519f4
```

## Running the tests

**Tests require `postgres` installed.**

`cargo test`
