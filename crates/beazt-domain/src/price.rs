//! The explorer's second driven port: a cached ZEC/USD price, read
//! synchronously — never a live fetch per page render. A sibling to
//! [`crate::ChainReader`], injected the same way at each driving adapter's
//! composition root.

/// Reads a cached ZEC/USD price. `None` means "no price available" —
/// callers render without a USD figure, never an error; a price source is
/// optional and cosmetic, not a dependency anything else degrades on.
pub trait PriceOracle: Send + Sync + 'static {
    /// The most recently cached ZEC/USD price, if any.
    fn zec_usd(&self) -> Option<f64>;
}

/// Always answers `None` — the default when no price source is
/// configured. Keeps "the zainod RPC URL is the only network dependency"
/// true by default: an operator who never opts into a price source makes
/// zero extra network calls.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullPriceOracle;

impl PriceOracle for NullPriceOracle {
    fn zec_usd(&self) -> Option<f64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_price_oracle_always_answers_none() {
        assert_eq!(NullPriceOracle.zec_usd(), None);
    }
}
