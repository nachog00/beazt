# beazt

A dogfood block explorer (web + TUI) over [zaino](https://github.com/zingolabs/zaino)'s
node-rpc surface. Extracted from zaino's own `feat/block-explorer` branch,
where it was built and iterated as a reference client proving zaino's
JSON-RPC wire contract end to end.

Not a competitor to NightHawk, CipherScan, or any other Zcash explorer —
the point was always dogfooding zaino's own RPC surface against a live
zainod, catching real wire-contract bugs along the way (two were found and
fixed upstream in `zaino-noderpc` during this project), and serving as a
reference client for zaino's node-rpc methods as they land.

## Architecture

Ports-and-adapters, five crates under `crates/`:

- **`beazt-domain`** — the two driven ports: `ChainReader` (zero transport
  dependency, not even on `zaino-noderpc`) and `PriceOracle` (a cached
  ZEC/USD price, optional — `NullPriceOracle` is the default). Also owns
  the pure zat/ZEC/USD/timestamp/thousands-grouping formatting helpers
  both driving adapters share.
- **`beazt-zaino-client`** — the one production `ChainReader`: wraps
  `zaino-noderpc`'s generated jsonrpsee client. Plain, no caching or retry.
- **`beazt-price-client`** — the one production `PriceOracle`: a
  background-refreshed cache backed by CoinGecko's free API. The only
  crate that gives this app a second external network dependency, and
  only when a composition root opts into one.
- **`beazt-web`** — a driving adapter: server-rendered web surface (axum +
  maud), generic over `ChainReader`.
- **`beazt-tui`** — a driving adapter: terminal surface (ratatui +
  crossterm), generic over `ChainReader`.

Each driving adapter's composition root (`src/bin/*.rs`) is the only place
that knows the concrete `ZainoClient`/`CoinGeckoPriceOracle` adapter types;
the library half of each (`src/lib.rs`) never does.

## Capability coverage

Covers essentially every RPC `zaino-noderpc`'s `NodeRpcApi` trait serves:
chain height, recent blocks, block detail + navigation, transaction detail
with disambiguated spent/unspent output status (`getspentinfo` +
`gettxout` together — neither alone can tell "unspent" apart from
"unknown"), per-block transparent value movements (`getblockdeltas`, a
differentiator Zebra itself doesn't serve), shielded treestate +
subtree roots, mempool listing, address balance/history/UTXOs/deltas,
address validation + unified-address receiver decomposition, node
diagnostics (mining/network/peers), richer blockchain info, and validator
ping.

Explicitly out of scope: `getchaintips`/`getblocksubsidy`/
`gettxoutsetinfo` aren't in `zaino-noderpc`'s trait yet on the branch this
was extracted from — wiring those needs new server-side handlers in zaino
itself, a separate task from what this client can reach into.

## Running it

```
# web
ZAINO_RPC_URL=http://127.0.0.1:8232 EXPLORER_ADDR=127.0.0.1:3000 \
  cargo run --bin web -p beazt-web

# TUI
ZAINO_RPC_URL=http://127.0.0.1:8232 \
  cargo run --bin tui -p beazt-tui
```

Both talk to any zainod's JSON-RPC endpoint; neither needs anything beyond
that one URL. Set `BEAZT_USD_PRICE=1` (either binary) to additionally opt
into a live CoinGecko-backed ZEC/USD price next to every amount — unset by
default, so the RPC URL stays the only network dependency unless you ask
for the second one.

## Logging

Every RPC failure logs a `warn` event in `beazt-zaino-client` — the one
place every `ChainReadError` actually originates — regardless of which
surface triggered it or whether that surface degrades the failure to an
empty/default rather than showing an error. `RUST_LOG` (default `info`)
controls verbosity on both binaries. `beazt-web` logs to stdout; `beazt-tui`
logs to `BEAZT_LOG_DIR`/`beazt-tui.log` (`BEAZT_LOG_DIR` defaults to `.`)
rather than stdout/stderr, since those are the terminal's own canvas while
the TUI's alternate screen is active.

## Testing

`cargo test --workspace`. Every capability has at least a mock-jsonrpsee-
server end-to-end test (`zaino_service::testing::{MockChain,
MockIndexerService}`, re-exported transitively via the `zaino-service` git
dependency below), plus pure-function mapping tests where the wire shape
warrants one.

## Dependency on zaino

`beazt-zaino-client` is the only crate with a real (non-dev) dependency on
`zaino-noderpc`; `zaino-service`/`zaino-primitives` appear only as
dev-dependencies, for building mock-server test fixtures. None of this is
published to crates.io, so all three are pinned as git dependencies to a
specific commit on zaino's `feat/block-explorer` branch (see the root
`Cargo.toml`) — re-pin to `dev` once this work (or the pieces of it zaino
wants) lands there.

The `[patch.crates-io]` block for `zebra-chain`/`zebra-rpc`/`zebra-state`
is load-bearing, not optional: zaino's own crates depend on that Zebra
fork rather than the public release, and this repo inherits that
requirement transitively. It must track whatever zaino's own `Cargo.toml`
pins at the same commit.

## History

The original design rationale lives at
`docs/superpowers/specs/2026-10-08-zaino-block-explorer-design.md`,
carried over from the source branch. Git history here is a filtered
extraction of exactly the commits that touched these four crates (plus
`Dockerfile.explorer` and that design doc) from zaino's `feat/block-explorer`
branch — nothing else from zaino's own history is included.
