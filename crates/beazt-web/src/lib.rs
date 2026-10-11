//! Server-rendered web surface over the explorer's domain. See
//! `docs/superpowers/specs/2026-10-08-zaino-block-explorer-design.md`.
//!
//! `build_app` and its routes are generic over [`ChainReader`], not over any
//! transport detail — nothing in this module knows a concrete adapter or
//! jsonrpsee exists. The composition root that injects one lives in
//! `src/bin/web.rs` alone.
#![forbid(unsafe_code)]

use axum::extract::{Path, State};
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use beazt_domain::ChainReader;
use maud::{html, Markup, PreEscaped, DOCTYPE};

/// The explorer's web router: every route is a live read through `reader`,
/// never a cached or locally re-derived value — that's the whole point of a
/// dogfood client.
pub fn build_app<C: ChainReader>(reader: C) -> Router {
    Router::new()
        .route("/", get(home::<C>))
        .route("/block/:id", get(block::<C>))
        .route("/block/:id/treestate", get(treestate::<C>))
        .route("/mempool", get(mempool::<C>))
        .route("/node", get(node::<C>))
        .route("/subtrees/:pool/:start_index", get(subtree_roots::<C>))
        .route("/tx/:txid", get(transaction::<C>))
        .route("/address/:address", get(address::<C>))
        .with_state(reader)
}

/// How many recent blocks the home page lists.
const RECENT_BLOCKS: u32 = 10;

/// Pico's classless theme (vendored, not fetched at request time — this
/// stays a single self-contained binary with no runtime network
/// dependency beyond the one RPC URL). It styles bare semantic elements
/// (`nav`, `table`, `h1`/`h2`, `a`, ...) directly, which is why every
/// route below renders plain tags with only a handful of extra classes.
const PICO_CSS: &str = include_str!("../assets/pico.classless.min.css");

/// The handful of semantic classes this app's own markup uses that Pico
/// has no opinion on, layered on top of Pico's CSS variables so they
/// still track its (auto) light/dark theme.
const EXTRA_CSS: &str = r#"
p.error { color: var(--pico-del-color); }
p.ok { color: var(--pico-ins-color); }
p.empty, span.empty { color: var(--pico-muted-color); }
.hash { font-family: var(--pico-font-family-monospace, monospace); word-break: break-all; }
nav a[href="/"] { font-weight: bold; }
"#;

/// Wraps route content in the shared page shell: doctype, nav, and the
/// embedded stylesheet. Every route's rendered markup passes through here.
fn layout(title: &str, content: Markup) -> Html<String> {
    let markup = html! {
        (DOCTYPE)
        html {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " — beazt" }
                style { (PreEscaped(PICO_CSS)) (PreEscaped(EXTRA_CSS)) }
            }
            body {
                nav {
                    ul {
                        li { a href="/" { strong { "beazt" } } }
                    }
                    ul {
                        li { a href="/mempool" { "Mempool" } }
                        li { a href="/node" { "Node" } }
                    }
                }
                main { (content) }
            }
        }
    };
    Html(markup.into_string())
}

/// `GET /`: the chain height, node/mempool status, and a recent-blocks
/// list, all fetched live and rendered independently of each other.
async fn home<C: ChainReader>(State(reader): State<C>) -> Html<String> {
    let height = reader.chain_height().await;
    let blocks = reader.recent_blocks(RECENT_BLOCKS).await;
    let status = reader.node_status().await;

    let content = html! {
        h1 { "Chain" }
        @match height {
            Ok(height) => p { "Chain height: " strong { (height) } },
            Err(e) => p.error { "RPC error: " (e.to_string()) },
        }
        @match status {
            Ok(status) => p {
                "Connected validator: " (status.subversion) " (build " (status.build) ") — "
                (status.connections) " peers — "
                a href="/mempool" { "mempool" } ": "
                (status.mempool_size) " tx, " (status.mempool_bytes) " bytes"
                " — " a href="/node" { "node info" }
            },
            Err(e) => p.error { "Node status unavailable: " (e.to_string()) },
        }
        h2 { "Recent blocks" }
        @match blocks {
            Ok(blocks) => table {
                thead { tr { th { "Height" } th { "Hash" } th { "Time" } th { "Tx" } } }
                tbody {
                    @for block in &blocks {
                        tr {
                            td { a href=(format!("/block/{}", block.height)) { (block.height) } }
                            td.hash { (block.hash) }
                            td { (block.time) }
                            td { (block.tx_count) }
                        }
                    }
                }
            },
            Err(e) => p.error { "RPC error: " (e.to_string()) },
        }
    };
    layout("Home", content)
}

/// `GET /block/{id}`: one block's detail — hash, time, every transaction id
/// it contains (each linked to `/tx/{txid}`), and every transparent value
/// movement within it — by height or hash (`getblock`'s own polymorphic id
/// parameter). Linked from the home page's block list. The value-movements
/// read is by the block's resolved hash (`getblockdeltas` takes no height
/// form) and renders independently of the transaction list, so a failure
/// there doesn't hide the rest of the page.
async fn block<C: ChainReader>(State(reader): State<C>, Path(id): Path<String>) -> Html<String> {
    let content = match reader.block(id.clone()).await {
        Ok(detail) => {
            let deltas = reader.block_deltas(detail.hash.clone()).await;
            html! {
                h1 { "Block " (detail.height) }
                article {
                    p { "Hash: " span.hash { (detail.hash) } }
                    p { "Time: " (detail.time) }
                    p { a href=(format!("/block/{id}/treestate")) { "Treestate" } }
                }
                h2 { "Transactions" }
                table {
                    thead { tr { th { "Tx ID" } } }
                    tbody {
                        @for txid in &detail.tx_ids {
                            tr { td.hash { a href=(format!("/tx/{txid}")) { (txid) } } }
                        }
                    }
                }
                h2 { "Value movements" }
                @match deltas {
                    Ok(block_deltas) => table {
                        thead { tr { th { "Tx ID" } th { "Value" } th { "Address" } } }
                        tbody {
                            @for delta in &block_deltas.deltas {
                                @for movement in delta.inputs.iter().chain(delta.outputs.iter()) {
                                    tr {
                                        td.hash { a href=(format!("/tx/{}", delta.txid)) { (delta.txid) } }
                                        td { (movement.value_zat) " zat" }
                                        td {
                                            @if let Some(address) = &movement.address {
                                                (address)
                                            } @else {
                                                span.empty { "—" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    Err(e) => p.error { "RPC error: " (e.to_string()) },
                }
            }
        }
        Err(e) => html! {
            h1 { "Block " (id) }
            p.error { "RPC error: " (e.to_string()) }
        },
    };
    layout(&format!("Block {id}"), content)
}

/// `GET /block/{id}/treestate`: a block's shielded commitment-tree state,
/// by pool — each active pool's tree root, plus a truncated preview of its
/// serialized state (which can be large) noting the full length. Linked
/// from the block page; kept as its own route rather than folded into it
/// so a large tree doesn't bloat every block-page load.
async fn treestate<C: ChainReader>(
    State(reader): State<C>,
    Path(id): Path<String>,
) -> Html<String> {
    let content = match reader.treestate(id.clone()).await {
        Ok(state) => html! {
            h1 { "Treestate for block " (state.height) }
            p { "Hash: " span.hash { (state.hash) } }
            p { "Time: " (state.time) }
            @for (label, pool, tree) in [
                ("Sapling", "sapling", &state.sapling),
                ("Orchard", "orchard", &state.orchard),
                ("Ironwood", "ironwood", &state.ironwood),
            ] {
                h2 { (label) }
                @match tree {
                    Some(tree) => article {
                        @match &tree.final_root {
                            Some(root) => p { "Final root: " span.hash { (root) } },
                            None => p { "Final root: unavailable" },
                        }
                        p { "Final state: " span.hash { (truncate_hex(&tree.final_state)) } }
                        p { a href=(format!("/subtrees/{pool}/0")) { "Subtree roots" } }
                    },
                    None => p.empty { "Not active at this block." },
                }
            }
        },
        Err(e) => html! {
            h1 { "Treestate for block " (id) }
            p.error { "RPC error: " (e.to_string()) }
        },
    };
    layout(&format!("Treestate {id}"), content)
}

/// `GET /subtrees/{pool}/{start_index}`: a contiguous run of complete
/// note-commitment subtree roots for a pool, from `start_index`. Linked
/// from the treestate page's active-pool sections. A `start_index` past
/// the end of the pool's completed subtrees renders an empty list, not
/// an error.
async fn subtree_roots<C: ChainReader>(
    State(reader): State<C>,
    Path((pool, start_index)): Path<(String, u16)>,
) -> Html<String> {
    let content = match reader.subtree_roots(pool.clone(), start_index).await {
        Ok(roots) => html! {
            h1 { "Subtree roots — " (roots.pool) " from index " (roots.start_index) }
            @if roots.roots.is_empty() {
                p.empty { "None (past the end of this pool's completed subtrees, or none yet)" }
            } @else {
                table {
                    thead { tr { th { "Root" } th { "Completed at height" } } }
                    tbody {
                        @for root in &roots.roots {
                            tr { td.hash { (root.root) } td { (root.end_height) } }
                        }
                    }
                }
            }
        },
        Err(e) => html! {
            h1 { "Subtree roots — " (pool) }
            p.error { "RPC error: " (e.to_string()) }
        },
    };
    layout("Subtree roots", content)
}

/// Truncates a long hex string for display, noting its full length — a
/// serialized commitment tree can be large and isn't meant to be read raw.
fn truncate_hex(hex: &str) -> String {
    const PREVIEW_LEN: usize = 64;
    if hex.len() <= PREVIEW_LEN {
        return hex.to_string();
    }
    format!("{}… ({} hex chars total)", &hex[..PREVIEW_LEN], hex.len())
}

/// `GET /tx/{txid}`: one transaction's detail, fetched live. Linked from a
/// block's transaction list and from an address's transaction history.
async fn transaction<C: ChainReader>(
    State(reader): State<C>,
    Path(txid): Path<String>,
) -> Html<String> {
    let content = match reader.transaction(txid.clone()).await {
        Ok(tx) => {
            let mut statuses = Vec::with_capacity(tx.outputs.len());
            for index in 0..tx.outputs.len() {
                let index =
                    u32::try_from(index).expect("more than u32::MAX outputs in one transaction");
                statuses.push(describe_output_status(&reader, &tx.txid, index).await);
            }
            html! {
                h1 { "Transaction " span.hash { (tx.txid) } }
                article {
                    p { "Size: " (tx.size) " bytes" }
                    @if let Some(height) = tx.height {
                        p { "Block height: " (height) }
                    }
                    @if let Some(confirmations) = tx.confirmations {
                        p { "Confirmations: " (confirmations) }
                    }
                }
                h2 { "Outputs" }
                table {
                    thead { tr { th { "Value" } th { "Addresses" } th { "Status" } } }
                    tbody {
                        @for (output, status) in tx.outputs.iter().zip(statuses.iter()) {
                            tr {
                                td { (output.value_zat) " zat" }
                                td {
                                    @if output.addresses.is_empty() {
                                        span.empty { "—" }
                                    } @else {
                                        (output.addresses.join(", "))
                                    }
                                }
                                td { (status) }
                            }
                        }
                    }
                }
            }
        }
        Err(e) => html! {
            h1 { "Transaction " (txid) }
            p.error { "RPC error: " (e.to_string()) }
        },
    };
    layout(&format!("Tx {txid}"), content)
}

/// Describes one output's status, combining two RPCs that each leave a
/// gap the other closes: `spend_info` names the spender when it knows
/// one, but answers "not found" for both a genuinely unspent output and
/// an unknown one; `output_status` (`gettxout`) answers the unspent case
/// with a value, but equally can't tell "spent" apart from "unknown".
/// Together they disambiguate all three states.
async fn describe_output_status<C: ChainReader>(reader: &C, txid: &str, index: u32) -> String {
    if let Ok(spend) = reader.spend_info(txid.to_string(), index).await {
        return format!("spent by {} in block {}", spend.spending_txid, spend.height);
    }
    match reader.output_status(txid.to_string(), index).await {
        Ok(Some(status)) => format!(
            "unspent — {} zat, {} confirmations",
            status.value_zat, status.confirmations
        ),
        Ok(None) => "spent or unknown".to_string(),
        Err(_) => "status unavailable".to_string(),
    }
}

/// `GET /address/{address}`: one transparent address's balance and
/// transaction history, fetched live. Not yet linked from anywhere — the
/// explorer has no search form yet, so this is reachable only by direct URL.
async fn address<C: ChainReader>(
    State(reader): State<C>,
    Path(address): Path<String>,
) -> Html<String> {
    // Validity is independent of the balance/txids/utxos/deltas reads
    // below — useful even for a shielded or unified address those reads
    // can't answer, and worth showing even if they fail.
    let validity = reader.validate_address(address.clone()).await;
    let validity_line = html! {
        @match &validity {
            Ok(v) if v.valid => p.ok {
                "Valid address"
                @if let Some(kind) = &v.kind {
                    " — " (kind)
                }
            },
            Ok(_) => p.empty { "Not a recognized address on this network" },
            Err(e) => p.error { "Validity unavailable: " (e.to_string()) },
        }
    };
    // z_listunifiedreceivers rejects any non-unified address as a
    // parameter error, so only call it once validity has actually said
    // "unified" — calling it on every address would just manufacture an
    // error on the common (transparent) case.
    let is_unified = matches!(&validity, Ok(v) if v.kind.as_deref() == Some("unified"));
    let receivers_section = if is_unified {
        match reader.list_receivers(address.clone()).await {
            Ok(receivers) => html! {
                h2 { "Receivers" }
                table {
                    tbody {
                        @if let Some(r) = &receivers.orchard { tr { td { "Orchard:" } td.hash { (r) } } }
                        @if let Some(r) = &receivers.sapling { tr { td { "Sapling:" } td.hash { (r) } } }
                        @if let Some(r) = &receivers.p2pkh { tr { td { "Transparent (P2PKH):" } td.hash { (r) } } }
                        @if let Some(r) = &receivers.p2sh { tr { td { "Transparent (P2SH):" } td.hash { (r) } } }
                    }
                }
            },
            Err(e) => html! { p.error { "Receivers unavailable: " (e.to_string()) } },
        }
    } else {
        html! {}
    };
    let content = match reader.address(address.clone()).await {
        Ok(summary) => html! {
            h1 { "Address " span.hash { (summary.address) } }
            (validity_line)
            (receivers_section)
            article {
                p { "Balance: " (summary.balance_zat) " zat" }
                p { "Lifetime received: " (summary.received_zat) " zat" }
            }
            h2 { "Transactions" }
            @if summary.txids.is_empty() {
                p.empty { "None" }
            } @else {
                table {
                    tbody {
                        @for txid in &summary.txids {
                            tr { td.hash { a href=(format!("/tx/{txid}")) { (txid) } } }
                        }
                    }
                }
            }
            h2 { "Unspent outputs" }
            @if summary.utxos.is_empty() {
                p.empty { "None (or unavailable on this deployment)" }
            } @else {
                table {
                    thead { tr { th { "Tx" } th { "Index" } th { "Value" } th { "Height" } } }
                    tbody {
                        @for utxo in &summary.utxos {
                            tr {
                                td.hash { a href=(format!("/tx/{}", utxo.txid)) { (utxo.txid) } }
                                td { (utxo.output_index) }
                                td { (utxo.value_zat) " zat" }
                                td { (utxo.height) }
                            }
                        }
                    }
                }
            }
            h2 { "Value changes" }
            @if summary.deltas.is_empty() {
                p.empty { "None (or unavailable on this deployment)" }
            } @else {
                table {
                    thead { tr { th { "Tx" } th { "Value" } th { "Height" } } }
                    tbody {
                        @for delta in &summary.deltas {
                            tr {
                                td.hash { a href=(format!("/tx/{}", delta.txid)) { (delta.txid) } }
                                td { (delta.value_zat) " zat" }
                                td { (delta.height) }
                            }
                        }
                    }
                }
            }
        },
        Err(e) => html! {
            h1 { "Address " (address) }
            (validity_line)
            (receivers_section)
            p.error { "RPC error: " (e.to_string()) }
        },
    };
    layout(&format!("Address {address}"), content)
}

/// `GET /mempool`: every transaction currently in the mempool, with its
/// size, fee, and the tip height it was validated against. Linked from the
/// home page's status line.
async fn mempool<C: ChainReader>(State(reader): State<C>) -> Html<String> {
    let content = match reader.raw_mempool().await {
        Ok(entries) => html! {
            h1 { "Mempool" }
            p { (entries.len()) " transactions" }
            @if entries.is_empty() {
                p.empty { "Nothing pending." }
            } @else {
                table {
                    thead { tr { th { "Tx" } th { "Size" } th { "Fee" } th { "Height" } th { "Time" } } }
                    tbody {
                        @for entry in &entries {
                            tr {
                                td.hash { a href=(format!("/tx/{}", entry.txid)) { (entry.txid) } }
                                td { (entry.size) " bytes" }
                                td { (entry.fee_zat) " zat" }
                                td { (entry.height) }
                                td {
                                    @if let Some(time) = entry.time {
                                        (time)
                                    } @else {
                                        span.empty { "—" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
        Err(e) => html! {
            h1 { "Mempool" }
            p.error { "RPC error: " (e.to_string()) }
        },
    };
    layout("Mempool", content)
}

/// `GET /node`: richer node diagnostics than the home page's compact status
/// line — mining/network facts and every connected peer. Linked from the
/// home page's status line.
async fn node<C: ChainReader>(State(reader): State<C>) -> Html<String> {
    // Both independent of node_diagnostics below — their own failure
    // doesn't hide mining/network/peer facts that answered.
    let blockchain = reader.blockchain_info().await;
    let ping = reader.ping().await;
    let ping_line = html! {
        p {
            "Validator: "
            @match ping {
                Ok(()) => "responsive",
                Err(_) => "unresponsive",
            }
        }
    };
    let blockchain_section = html! {
        (ping_line)
        h2 { "Blockchain" }
        @match &blockchain {
            Ok(info) => article {
                p { "Chain: " (info.chain) }
                p { "Blocks: " (info.blocks) " — headers: " (info.headers) }
                p { "Best block: " span.hash { (info.best_block_hash) } }
                p { "Difficulty: " (info.difficulty) }
                p { "Verification progress: " (info.verification_progress) }
                p { "Size on disk: " (info.size_on_disk) " bytes" }
                p { "Estimated network height: " (info.estimated_height) }
            },
            Err(e) => p.error { "RPC error: " (e.to_string()) },
        }
    };
    let content = match reader.node_diagnostics().await {
        Ok(diagnostics) => html! {
            h1 { "Node info" }
            (blockchain_section)
            h2 { "Mining" }
            @match &diagnostics.mining {
                Some(mining) => article {
                    p { "Chain: " (mining.chain) }
                    @if let Some(difficulty) = mining.difficulty {
                        p { "Difficulty: " (difficulty) }
                    }
                    @if let Some(sol_ps) = mining.network_sol_ps {
                        p { "Network solution rate: " (sol_ps) " sol/s" }
                    }
                },
                None => p.empty { "Unavailable on this deployment" },
            }
            h2 { "Network" }
            @match &diagnostics.network {
                Some(network) => article {
                    p { "Protocol version: " (network.protocol_version) }
                    p { "Local services: " (network.local_services) }
                    p { "Relay fee: " (network.relay_fee) " ZEC" }
                    @if !network.warnings.is_empty() {
                        p { "Warnings: " (network.warnings) }
                    }
                },
                None => p.empty { "Unavailable on this deployment" },
            }
            h2 { "Peers" }
            @if diagnostics.peers.is_empty() {
                p.empty { "None" }
            } @else {
                table {
                    thead { tr { th { "Address" } th { "Direction" } } }
                    tbody {
                        @for peer in &diagnostics.peers {
                            tr {
                                td.hash { (peer.addr) }
                                td { @if peer.inbound { "inbound" } @else { "outbound" } }
                            }
                        }
                    }
                }
            }
        },
        Err(e) => html! {
            h1 { "Node info" }
            (blockchain_section)
            p.error { "RPC error: " (e.to_string()) }
        },
    };
    layout("Node info", content)
}

#[cfg(test)]
mod tests {
    use beazt_zaino_client::ZainoClient;
    use http_body_util::BodyExt;
    use jsonrpsee::http_client::HttpClientBuilder;
    use std::net::TcpListener;
    use tower::ServiceExt;
    use zaino_noderpc::{NodeRpc, NodeRpcApiServer};
    use zaino_service::testing::{MockChain, MockIndexerService};
    use zcash_protocol::consensus::Network;

    /// Boot a real `NodeRpc` jsonrpsee server on an ephemeral port, over the
    /// given mock chain, and return its address and a handle the test stops
    /// when done.
    async fn spawn_mock_server(
        chain: MockChain,
    ) -> (std::net::SocketAddr, jsonrpsee::server::ServerHandle) {
        let handler = NodeRpc::new(MockIndexerService::new(chain), Network::MainNetwork);
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        listener.set_nonblocking(true).expect("set nonblocking");
        let addr = listener.local_addr().expect("local addr");
        let server = jsonrpsee::server::ServerBuilder::default()
            .build_from_tcp(listener)
            .expect("build server from listener");
        let handle = server.start(handler.into_rpc());
        (addr, handle)
    }

    /// A mock chain tipped at height 291 with nothing else scripted — the
    /// baseline most route tests use, where only the height itself resolves
    /// and everything else (blocks, transactions, addresses) correctly
    /// reports an RPC error.
    fn tip_only_chain() -> MockChain {
        use zaino_primitives::types::{BlockHash, BlockRef, Height};

        MockChain {
            tip: Some(BlockRef {
                height: Height::try_from(291).expect("valid height"),
                hash: BlockHash::from([0xCDu8; 32]),
            }),
            ..Default::default()
        }
    }

    /// The home page renders the mock chain's height (291) fetched live
    /// through the generated client — not a cached or hand-rolled value. The
    /// router here is generic over `ChainReader`; this test is what proves
    /// that generic code actually works end-to-end with the one real
    /// implementor. `getblock` isn't scripted on this mock (only the tip
    /// is), so the blocks section is expected to report an RPC error rather
    /// than panic or render silently — proving height and blocks render
    /// independently of each other, and that the error path is wired, not
    /// just the happy path.
    //
    // multi_thread required: the test drives a live jsonrpsee server (its own
    // accept loop) concurrently with the router's outbound RPC call to it, on
    // one runtime — see the identical justification in zaino-noderpc's own
    // `transport.rs` server tests.
    #[tokio::test(flavor = "multi_thread")]
    async fn home_page_renders_block_rpc_error_without_losing_the_height() {
        let (addr, handle) = spawn_mock_server(tip_only_chain()).await;
        let client = HttpClientBuilder::default()
            .build(format!("http://{addr}"))
            .expect("build http client");
        let app = crate::build_app(ZainoClient::new(client));

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router does not error");

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("read body")
            .to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("utf8 body");
        assert!(text.contains("291"), "height still renders: {text}");
        assert!(
            text.contains("RPC error"),
            "unscripted getblock should render as an RPC error, not silence: {text}"
        );

        let _ = handle.stop();
    }

    /// `/block/{id}` for an unscripted height renders a graceful RPC error,
    /// not a panic or a 500 — the route is reachable and its error path is
    /// wired. The happy-path mapping (`BlockDetail` field-for-field,
    /// including txids) is covered in `beazt-zaino-client`'s own
    /// pure-function and mock-server tests; this test is about the route.
    #[tokio::test(flavor = "multi_thread")]
    async fn block_route_renders_rpc_error_for_an_unscripted_height() {
        let (addr, handle) = spawn_mock_server(tip_only_chain()).await;
        let client = HttpClientBuilder::default()
            .build(format!("http://{addr}"))
            .expect("build http client");
        let app = crate::build_app(ZainoClient::new(client));

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/block/1")
                    .body(axum::body::Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router does not error");

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("read body")
            .to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("utf8 body");
        assert!(
            text.contains("RPC error"),
            "an unscripted height should render as an RPC error, not a panic: {text}"
        );

        let _ = handle.stop();
    }

    /// `/block/{id}` for a fully-scripted block renders its value
    /// movements — a negative input and a positive output, each with its
    /// address — alongside the transaction list. The differentiator
    /// capability (`getblockdeltas`) rendering end to end, not just the
    /// pure-function mapping `beazt-zaino-client` already covers.
    #[tokio::test(flavor = "multi_thread")]
    async fn block_route_renders_value_movements() {
        use zaino_primitives::types::{
            Block, BlockHash, BlockHeader, BlockRef, BlockTreeSizes, BlockVerbose, ChainMetadata,
            CompactDifficulty, DecodedBlock, EquihashSolution, Height, Script, SignedZatoshis,
            TransactionId, Zatoshis,
        };
        use zaino_service::{InputDelta, OutputDelta, TransactionDeltas};

        let header = BlockHeader {
            hash: BlockHash::from([0x11; 32]),
            version: 4,
            prev_hash: BlockHash::from([0x22; 32]),
            height: Height::try_from(300).expect("valid height"),
            time: 1_700_000_300,
            merkle_root: [0x33; 32].into(),
            block_commitments: [0x44; 32].into(),
            bits: CompactDifficulty::try_from_bits(0x2007_ffff).expect("valid nBits"),
            nonce: [0x55; 32],
            solution: EquihashSolution::Regtest([0; 36]),
        };
        let p2pkh = |b: u8| {
            let mut bytes = vec![0x76, 0xa9, 0x14];
            bytes.extend_from_slice(&[b; 20]);
            bytes.extend_from_slice(&[0x88, 0xac]);
            Script::new(bytes)
        };
        let spend = TransactionDeltas {
            txid: TransactionId::from([0x7A; 32]),
            index: 0,
            inputs: vec![InputDelta {
                script: p2pkh(0x02),
                satoshis: SignedZatoshis::try_new(-1_000).expect("valid amount"),
                index: 0,
                prev_txid: TransactionId::from([0xAB; 32]),
                prevout: 2,
            }],
            outputs: vec![OutputDelta {
                script: p2pkh(0x03),
                satoshis: Zatoshis::new(600).expect("valid amount"),
                index: 0,
            }],
        };
        let chain = MockChain {
            tip: Some(BlockRef {
                height: Height::try_from(300).expect("valid height"),
                hash: BlockHash::from([0x11; 32]),
            }),
            block: Some(Block {
                header,
                transactions: Vec::new(),
                chain_metadata: ChainMetadata::ZERO,
            }),
            block_verbose: Some(BlockVerbose {
                confirmations: 1,
                difficulty: 1.0,
                chainwork: None,
                chain_supply: None,
                value_pools: Vec::new(),
                final_sapling_root: None,
                final_orchard_root: None,
                tree_sizes: BlockTreeSizes::default(),
                next_block_hash: None,
            }),
            decoded_block: Some(DecodedBlock {
                size: 1_000,
                transactions: Vec::new(),
            }),
            block_deltas: Some(zaino_service::BlockDeltas {
                hash: BlockHash::from([0x11; 32]),
                confirmations: 1,
                size: 500,
                height: Height::try_from(300).expect("valid height"),
                version: 4,
                merkle_root: [0x22; 32].into(),
                deltas: vec![spend],
                time: 1_700_000_300,
                median_time: 1_700_000_000,
                nonce: [0x33; 32],
                bits: CompactDifficulty::try_from_bits(0x2007_ffff).expect("valid nBits"),
                difficulty: 1.0,
                chainwork: None,
                prev_hash: None,
                next_hash: None,
            }),
            ..Default::default()
        };
        let (addr, handle) = spawn_mock_server(chain).await;
        let client = HttpClientBuilder::default()
            .build(format!("http://{addr}"))
            .expect("build http client");
        let app = crate::build_app(ZainoClient::new(client));

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/block/300")
                    .body(axum::body::Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router does not error");

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("read body")
            .to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("utf8 body");
        assert!(text.contains("-1000 zat"), "{text}");
        assert!(text.contains("600 zat"), "{text}");
        assert!(text.contains("Value movements"), "{text}");

        let _ = handle.stop();
    }

    /// `/block/{id}/treestate` for an unscripted block renders a graceful
    /// RPC error, not a panic — the route is reachable and its error path
    /// is wired. The happy-path mapping is covered in
    /// `beazt-zaino-client`'s own tests; `truncate_hex` is covered
    /// directly below.
    #[tokio::test(flavor = "multi_thread")]
    async fn treestate_route_renders_rpc_error_for_an_unscripted_block() {
        let (addr, handle) = spawn_mock_server(tip_only_chain()).await;
        let client = HttpClientBuilder::default()
            .build(format!("http://{addr}"))
            .expect("build http client");
        let app = crate::build_app(ZainoClient::new(client));

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/block/1/treestate")
                    .body(axum::body::Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router does not error");

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("read body")
            .to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("utf8 body");
        assert!(
            text.contains("RPC error"),
            "an unscripted block should render as an RPC error, not a panic: {text}"
        );

        let _ = handle.stop();
    }

    /// A hex string within the preview length renders in full; a longer one
    /// truncates and notes the full length rather than printing it raw.
    #[test]
    fn truncate_hex_leaves_short_strings_alone_and_truncates_long_ones() {
        assert_eq!(super::truncate_hex("deadbeef"), "deadbeef");
        let long = "ab".repeat(100);
        let truncated = super::truncate_hex(&long);
        assert!(truncated.len() < long.len());
        assert!(truncated.contains("200 hex chars total"));
    }

    /// `/mempool` for an empty mock mempool renders zero transactions, not
    /// an error — an empty mempool is routine, not a failure. Proves the
    /// route is reachable; the field-mapping itself is covered by
    /// `beazt-zaino-client`'s own tests.
    #[tokio::test(flavor = "multi_thread")]
    async fn mempool_route_renders_zero_transactions_for_an_empty_mempool() {
        let (addr, handle) = spawn_mock_server(tip_only_chain()).await;
        let client = HttpClientBuilder::default()
            .build(format!("http://{addr}"))
            .expect("build http client");
        let app = crate::build_app(ZainoClient::new(client));

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/mempool")
                    .body(axum::body::Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router does not error");

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("read body")
            .to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("utf8 body");
        assert!(
            text.contains("0 transactions"),
            "an empty mempool should read as zero, not an error: {text}"
        );

        let _ = handle.stop();
    }

    /// `/node` renders a graceful RPC error when `getmininginfo` isn't
    /// scripted (the mock always answers `NotReady` for it, mirroring
    /// `node_status`'s own dependency) — not a panic. The happy-path
    /// mapping is covered in `beazt-zaino-client`'s own
    /// pure-function test; this test is about the route.
    #[tokio::test(flavor = "multi_thread")]
    async fn node_route_renders_unavailable_sections_when_not_ready() {
        let (addr, handle) = spawn_mock_server(tip_only_chain()).await;
        let client = HttpClientBuilder::default()
            .build(format!("http://{addr}"))
            .expect("build http client");
        let app = crate::build_app(ZainoClient::new(client));

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/node")
                    .body(axum::body::Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router does not error");

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("read body")
            .to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("utf8 body");
        // Mining and network both fail independently on this mock
        // (neither is scripted), so both sections report "unavailable"
        // rather than the whole page erroring — this is the fix for the
        // real bug the live deployment surfaced (getnetworkinfo's gap
        // was hiding getmininginfo's and getpeerinfo's working data).
        assert!(
            text.contains("Unavailable on this deployment"),
            "an unready node's sections should degrade, not error the whole page: {text}"
        );

        let _ = handle.stop();
    }

    /// `/node` renders the blockchain section even when only `tip` is
    /// scripted — `blockchain_info`'s own mock synthesizes an aggregate
    /// from the tip when nothing richer is scripted, mirroring a real
    /// zainod with no validator behind it yet.
    #[tokio::test(flavor = "multi_thread")]
    async fn node_route_renders_blockchain_section() {
        let (addr, handle) = spawn_mock_server(tip_only_chain()).await;
        let client = HttpClientBuilder::default()
            .build(format!("http://{addr}"))
            .expect("build http client");
        let app = crate::build_app(ZainoClient::new(client));

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/node")
                    .body(axum::body::Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router does not error");

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("read body")
            .to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("utf8 body");
        assert!(text.contains("Blockchain"), "{text}");
        assert!(text.contains("Blocks: 291"), "{text}");
        assert!(
            text.contains("Validator: responsive"),
            "the mock always answers ping: {text}"
        );

        let _ = handle.stop();
    }

    /// `/subtrees/{pool}/{start_index}` for an unscripted pool renders an
    /// empty list, not an error — the mock has no roots scripted, which
    /// is a legitimate "none yet" answer, not a failure.
    #[tokio::test(flavor = "multi_thread")]
    async fn subtree_roots_route_renders_empty_list_for_an_unscripted_pool() {
        let (addr, handle) = spawn_mock_server(tip_only_chain()).await;
        let client = HttpClientBuilder::default()
            .build(format!("http://{addr}"))
            .expect("build http client");
        let app = crate::build_app(ZainoClient::new(client));

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/subtrees/sapling/0")
                    .body(axum::body::Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router does not error");

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("read body")
            .to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("utf8 body");
        assert!(
            text.contains("None (past the end"),
            "an unscripted pool should read as empty, not an error: {text}"
        );

        let _ = handle.stop();
    }

    /// `/tx/{txid}` for an unscripted txid renders a graceful RPC error
    /// (zcashd's own "not found" for an unknown transaction), not a panic
    /// or a 500 — the route is reachable and its error path is wired. The
    /// happy-path mapping (`TransactionDetail` field-for-field) is covered
    /// in `beazt-zaino-client`'s own pure-function tests, which
    /// don't need a server; this test is about the route, not the mapping.
    #[tokio::test(flavor = "multi_thread")]
    async fn transaction_route_renders_rpc_error_for_an_unknown_txid() {
        let (addr, handle) = spawn_mock_server(tip_only_chain()).await;
        let client = HttpClientBuilder::default()
            .build(format!("http://{addr}"))
            .expect("build http client");
        let app = crate::build_app(ZainoClient::new(client));

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!("/tx/{}", "ab".repeat(32)))
                    .body(axum::body::Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router does not error");

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("read body")
            .to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("utf8 body");
        assert!(
            text.contains("RPC error"),
            "an unscripted txid should render as an RPC error, not a panic: {text}"
        );

        let _ = handle.stop();
    }

    /// `/address/{address}` for an address with no scripted history renders
    /// a zero balance, not an error — explorer policy reads "nothing found"
    /// as zero, distinct from an actual RPC failure. Proves the route is
    /// reachable and composes both underlying calls (balance + txids)
    /// without crashing; the field-mapping itself is covered by
    /// `beazt-zaino-client`'s own test against a scripted balance.
    #[tokio::test(flavor = "multi_thread")]
    async fn address_route_renders_zero_balance_for_an_unknown_address() {
        let (addr, handle) = spawn_mock_server(tip_only_chain()).await;
        let client = HttpClientBuilder::default()
            .build(format!("http://{addr}"))
            .expect("build http client");
        let app = crate::build_app(ZainoClient::new(client));

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/address/t1unknown")
                    .body(axum::body::Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router does not error");

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("read body")
            .to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("utf8 body");
        assert!(
            text.contains("Balance: 0 zat"),
            "an address with no history should read as zero, not an error: {text}"
        );

        let _ = handle.stop();
    }

    /// `/address/{address}` for a well-formed mainnet transparent address
    /// renders its validity and kind, alongside the zero balance — the
    /// two reads compose independently. Pure function of the address and
    /// network, so no chain state needs scripting.
    #[tokio::test(flavor = "multi_thread")]
    async fn address_route_renders_validity() {
        let (addr, handle) = spawn_mock_server(tip_only_chain()).await;
        let client = HttpClientBuilder::default()
            .build(format!("http://{addr}"))
            .expect("build http client");
        let app = crate::build_app(ZainoClient::new(client));

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/address/t1VTjv7XF3hYqxQkxKmHHErvus3bDrbbkGg")
                    .body(axum::body::Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router does not error");

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("read body")
            .to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("utf8 body");
        assert!(text.contains("Valid address"), "{text}");
        assert!(text.contains("p2pkh"), "{text}");

        let _ = handle.stop();
    }

    /// `/address/{address}` for a real mainnet unified address renders its
    /// bundled receivers — the route only calls `z_listunifiedreceivers`
    /// once validity has said "unified", so this also proves that gating
    /// doesn't misfire on the one kind it should fire for.
    #[tokio::test(flavor = "multi_thread")]
    async fn address_route_renders_receivers_for_a_unified_address() {
        let (addr, handle) = spawn_mock_server(tip_only_chain()).await;
        let client = HttpClientBuilder::default()
            .build(format!("http://{addr}"))
            .expect("build http client");
        let app = crate::build_app(ZainoClient::new(client));

        let ua = "u1pg2aaph7jp8rpf6yhsza25722sg5fcn3vaca6ze27hqjw7jvvhhuxkpcg0ge9xh6\
                  drsgdkda8qjq5chpehkcpxf87rnjryjqwymdheptpvnljqqrjqzjwkc2ma6hcq666k\
                  gwfytxwac8eyex6ndgr6ezte66706e3vaqrd25dzvzkc69kw0jgywtd0cmq52q5lkw\
                  6uh7hyvzjse8ksx";
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!("/address/{ua}"))
                    .body(axum::body::Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router does not error");

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("read body")
            .to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("utf8 body");
        assert!(text.contains("unified"), "{text}");
        assert!(text.contains("Receivers"), "{text}");
        assert!(text.contains("Orchard:"), "{text}");

        let _ = handle.stop();
    }

    /// `/address/{address}` with scripted UTXOs and deltas renders both
    /// sections, each entry linked to its transaction.
    #[tokio::test(flavor = "multi_thread")]
    async fn address_route_renders_utxos_and_deltas() {
        use zaino_primitives::types::{
            AddressDelta, BlockHash, BlockRef, Height, Script, SignedZatoshis, TransactionId,
            TransparentAddress, Utxo, Zatoshis,
        };

        let chain = MockChain {
            tip: Some(BlockRef {
                height: Height::try_from(300).expect("valid height"),
                hash: BlockHash::from([0x09; 32]),
            }),
            utxos: vec![Utxo {
                address: TransparentAddress::new("t1exampleaddress".to_string()),
                txid: TransactionId::from([0x04; 32]),
                output_index: 0,
                script: Script::new(vec![0x76, 0xa9]),
                satoshis: Zatoshis::new(5_000).expect("valid amount"),
                height: Height::try_from(300).expect("valid height"),
            }],
            deltas: vec![AddressDelta {
                satoshis: SignedZatoshis::try_new(5_000).expect("valid amount"),
                txid: TransactionId::from([0x04; 32]),
                index: 0,
                height: Height::try_from(300).expect("valid height"),
                address: TransparentAddress::new("t1exampleaddress".to_string()),
                block_index: None,
            }],
            ..Default::default()
        };
        let (addr, handle) = spawn_mock_server(chain).await;
        let client = HttpClientBuilder::default()
            .build(format!("http://{addr}"))
            .expect("build http client");
        let app = crate::build_app(ZainoClient::new(client));

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/address/t1exampleaddress")
                    .body(axum::body::Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router does not error");

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("read body")
            .to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("utf8 body");
        assert!(text.contains("Unspent outputs"), "{text}");
        assert!(text.contains("Value changes"), "{text}");
        assert!(text.contains(&"04".repeat(32)), "{text}");
        assert!(text.contains("5000 zat"), "{text}");

        let _ = handle.stop();
    }
}
