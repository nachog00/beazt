//! Composition root for the web surface: the one place that constructs the
//! concrete [`ZainoClient`] adapter and injects it into the generic router.
//!
//! Config: three env vars, nothing fancier. `ZAINO_RPC_URL` (default
//! `http://127.0.0.1:8232`) is the zainod node-rpc endpoint; `EXPLORER_ADDR`
//! (default `127.0.0.1:3000`) is where this binary listens; `BEAZT_USD_PRICE`
//! (unset by default) opts into a live CoinGecko-backed ZEC/USD price —
//! unset, every amount still renders in zatoshis/ZEC, this app's one
//! external dependency stays just the RPC URL.

use std::sync::Arc;
use std::time::Duration;

use beazt_domain::{NullPriceOracle, PriceOracle};
use beazt_price_client::CoinGeckoPriceOracle;
use beazt_zaino_client::ZainoClient;
use jsonrpsee::http_client::HttpClientBuilder;

/// How often the CoinGecko-backed oracle refreshes its cached price, when
/// enabled. Cosmetic data — no need for anything tighter.
const PRICE_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

#[tokio::main]
async fn main() {
    let rpc_url =
        std::env::var("ZAINO_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8232".to_string());
    let listen_addr =
        std::env::var("EXPLORER_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".to_string());
    let prices: Arc<dyn PriceOracle> = if std::env::var("BEAZT_USD_PRICE").is_ok() {
        Arc::new(CoinGeckoPriceOracle::spawn(PRICE_REFRESH_INTERVAL))
    } else {
        Arc::new(NullPriceOracle)
    };

    let client = HttpClientBuilder::default()
        .build(&rpc_url)
        .unwrap_or_else(|e| panic!("invalid ZAINO_RPC_URL {rpc_url:?}: {e}"));
    let app = beazt_web::build_app(ZainoClient::new(client), prices);

    let listener = tokio::net::TcpListener::bind(&listen_addr)
        .await
        .unwrap_or_else(|e| panic!("bind {listen_addr:?}: {e}"));
    println!("beazt listening on http://{listen_addr}, talking to {rpc_url}");
    axum::serve(listener, app)
        .await
        .unwrap_or_else(|e| panic!("serve: {e}"));
}
