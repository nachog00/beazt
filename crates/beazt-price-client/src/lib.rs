//! A [`PriceOracle`] backed by CoinGecko's free public API. Not zaino
//! itself, and not required by anything else in this workspace — this is
//! the one crate that gives `beazt` a second external network
//! dependency, and only when a composition root actually constructs one.
#![forbid(unsafe_code)]

use std::sync::{Arc, RwLock};
use std::time::Duration;

use beazt_domain::PriceOracle;

const COINGECKO_SIMPLE_PRICE_URL: &str =
    "https://api.coingecko.com/api/v3/simple/price?ids=zcash&vs_currencies=usd";

/// A [`PriceOracle`] that refreshes a cached ZEC/USD price from CoinGecko
/// on a background interval. A failed refresh keeps the last known-good
/// value rather than clearing it — a transient API hiccup shouldn't make
/// a page that was showing a price start showing none.
#[derive(Clone)]
pub struct CoinGeckoPriceOracle {
    cached: Arc<RwLock<Option<f64>>>,
}

impl CoinGeckoPriceOracle {
    /// Spawns the background refresh task and returns immediately; the
    /// first read may answer `None` until the first fetch completes.
    pub fn spawn(refresh_interval: Duration) -> Self {
        let cached = Arc::new(RwLock::new(None));
        let oracle = Self {
            cached: cached.clone(),
        };
        tokio::spawn(async move {
            let client = reqwest::Client::new();
            loop {
                if let Some(price) = fetch_price(&client).await {
                    if let Ok(mut guard) = cached.write() {
                        *guard = Some(price);
                    }
                }
                tokio::time::sleep(refresh_interval).await;
            }
        });
        oracle
    }
}

impl PriceOracle for CoinGeckoPriceOracle {
    fn zec_usd(&self) -> Option<f64> {
        self.cached.read().ok().and_then(|guard| *guard)
    }
}

async fn fetch_price(client: &reqwest::Client) -> Option<f64> {
    let body = client
        .get(COINGECKO_SIMPLE_PRICE_URL)
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;
    parse_simple_price_response(&body)
}

/// Parses CoinGecko's `simple/price` response shape:
/// `{"zcash":{"usd":42.17}}`. A pure function so the parsing logic is
/// unit-testable without a live network call.
fn parse_simple_price_response(body: &str) -> Option<f64> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value.get("zcash")?.get("usd")?.as_f64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_well_formed_response() {
        let body = r#"{"zcash":{"usd":42.17}}"#;
        assert_eq!(parse_simple_price_response(body), Some(42.17));
    }

    #[test]
    fn returns_none_for_malformed_or_unexpected_shapes() {
        assert_eq!(parse_simple_price_response("not json"), None);
        assert_eq!(parse_simple_price_response(r#"{"other":{}}"#), None);
        assert_eq!(parse_simple_price_response(r#"{"zcash":{}}"#), None);
    }

    #[test]
    fn a_fresh_cache_answers_none() {
        let oracle = CoinGeckoPriceOracle {
            cached: Arc::new(RwLock::new(None)),
        };
        assert_eq!(oracle.zec_usd(), None);
    }

    #[test]
    fn a_cached_value_is_read_back() {
        let oracle = CoinGeckoPriceOracle {
            cached: Arc::new(RwLock::new(Some(42.17))),
        };
        assert_eq!(oracle.zec_usd(), Some(42.17));
    }
}
