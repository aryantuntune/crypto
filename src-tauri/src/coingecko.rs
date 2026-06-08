use crate::error::{AppError, Result};
use serde::Deserialize;
use std::time::Duration;

const BASE: &str = "https://api.coingecko.com/api/v3";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Map common ticker (BTC, ETH, BTCUSDT, etc) to CoinGecko id.
pub fn symbol_to_id(symbol: &str) -> Option<&'static str> {
    let s = symbol.to_uppercase();
    let s = s.trim_end_matches("USDT").trim_end_matches("USD").trim_end_matches("USDC");
    match s {
        // Existing top coins
        "BTC" => Some("bitcoin"),
        "ETH" => Some("ethereum"),
        "SOL" => Some("solana"),
        "BNB" => Some("binancecoin"),
        "XRP" => Some("ripple"),
        "ADA" => Some("cardano"),
        "DOGE" => Some("dogecoin"),
        "AVAX" => Some("avalanche-2"),
        "MATIC" => Some("matic-network"),
        "LINK" => Some("chainlink"),
        "DOT" => Some("polkadot"),
        "LTC" => Some("litecoin"),
        // Expanded coverage (~40 of the most-traded coins)
        "TRX" => Some("tron"),
        "TON" => Some("the-open-network"),
        "SHIB" => Some("shiba-inu"),
        "BCH" => Some("bitcoin-cash"),
        "NEAR" => Some("near"),
        "UNI" => Some("uniswap"),
        "APT" => Some("aptos"),
        "ICP" => Some("internet-computer"),
        "ETC" => Some("ethereum-classic"),
        "XLM" => Some("stellar"),
        "FIL" => Some("filecoin"),
        "ARB" => Some("arbitrum"),
        "OP" => Some("optimism"),
        "INJ" => Some("injective-protocol"),
        "SUI" => Some("sui"),
        "SEI" => Some("sei-network"),
        "ATOM" => Some("cosmos"),
        "XMR" => Some("monero"),
        "HBAR" => Some("hedera-hashgraph"),
        "VET" => Some("vechain"),
        "ALGO" => Some("algorand"),
        "AAVE" => Some("aave"),
        "MKR" => Some("maker"),
        "RENDER" => Some("render-token"),
        "RNDR" => Some("render-token"),
        "IMX" => Some("immutable-x"),
        "GRT" => Some("the-graph"),
        "STX" => Some("blockstack"),
        "FTM" => Some("fantom"),
        "RUNE" => Some("thorchain"),
        "PEPE" => Some("pepe"),
        "WIF" => Some("dogwifcoin"),
        "TIA" => Some("celestia"),
        "FLOW" => Some("flow"),
        "EGLD" => Some("elrond-erd-2"),
        "SAND" => Some("the-sandbox"),
        "MANA" => Some("decentraland"),
        "AXS" => Some("axie-infinity"),
        "THETA" => Some("theta-token"),
        "EOS" => Some("eos"),
        "XTZ" => Some("tezos"),
        "CHZ" => Some("chiliz"),
        "GALA" => Some("gala"),
        "CRV" => Some("curve-dao-token"),
        "LDO" => Some("lido-dao"),
        "SNX" => Some("havven"),
        "COMP" => Some("compound-governance-token"),
        "DYDX" => Some("dydx-chain"),
        "FET" => Some("fetch-ai"),
        "JUP" => Some("jupiter-exchange-solana"),
        "ENA" => Some("ethena"),
        "ORDI" => Some("ordinals"),
        "BONK" => Some("bonk"),
        "FLOKI" => Some("floki"),
        "KAS" => Some("kaspa"),
        "QNT" => Some("quant-network"),
        "GMX" => Some("gmx"),
        "1INCH" => Some("1inch"),
        "ENS" => Some("ethereum-name-service"),
        "ZEC" => Some("zcash"),
        "DASH" => Some("dash"),
        _ => None,
    }
}

#[derive(Deserialize)]
struct PriceResp(std::collections::HashMap<String, std::collections::HashMap<String, f64>>);

/// Returns true for errors that are worth retrying once (transient network or 5xx).
fn is_transient(err: &reqwest::Error) -> bool {
    if err.is_timeout() || err.is_connect() || err.is_request() {
        return true;
    }
    matches!(err.status().map(|s| s.is_server_error()), Some(true))
}

pub async fn get_usd_price(symbol: &str) -> Result<f64> {
    let id = symbol_to_id(symbol)
        .ok_or_else(|| AppError::Invalid(format!("unknown symbol for CoinGecko: {}", symbol)))?;
    let url = format!("{}/simple/price?ids={}&vs_currencies=usd", BASE, id);

    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()?;

    // Retry once on a transient network/5xx error before giving up.
    let mut last_err: Option<AppError> = None;
    for attempt in 0..2 {
        match client.get(&url).send().await {
            Ok(r) => {
                let status = r.status();
                if status.is_success() {
                    let body: PriceResp = r.json().await?;
                    let price = body
                        .0
                        .get(id)
                        .and_then(|m| m.get("usd"))
                        .copied()
                        .ok_or_else(|| AppError::Internal("price missing in response".into()))?;
                    return Ok(price);
                }
                // Retry once on 5xx; fail fast on other (4xx) statuses.
                if status.is_server_error() && attempt == 0 {
                    last_err = Some(AppError::Internal(format!("coingecko HTTP {}", status)));
                    continue;
                }
                return Err(AppError::Internal(format!("coingecko HTTP {}", status)));
            }
            Err(e) => {
                if is_transient(&e) && attempt == 0 {
                    last_err = Some(AppError::Http(e));
                    continue;
                }
                return Err(AppError::Http(e));
            }
        }
    }

    Err(last_err
        .unwrap_or_else(|| AppError::Internal("coingecko request failed".into())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_normalization() {
        assert_eq!(symbol_to_id("btc"), Some("bitcoin"));
        assert_eq!(symbol_to_id("BTCUSDT"), Some("bitcoin"));
        assert_eq!(symbol_to_id("ETHUSDC"), Some("ethereum"));
        assert_eq!(symbol_to_id("XYZ"), None);
    }

    #[test]
    fn new_symbols_resolve() {
        assert_eq!(symbol_to_id("TRX"), Some("tron"));
        assert_eq!(symbol_to_id("TON"), Some("the-open-network"));
        assert_eq!(symbol_to_id("SHIB"), Some("shiba-inu"));
        assert_eq!(symbol_to_id("BCH"), Some("bitcoin-cash"));
        assert_eq!(symbol_to_id("NEAR"), Some("near"));
        assert_eq!(symbol_to_id("UNI"), Some("uniswap"));
        assert_eq!(symbol_to_id("APT"), Some("aptos"));
        assert_eq!(symbol_to_id("ICP"), Some("internet-computer"));
        assert_eq!(symbol_to_id("ETC"), Some("ethereum-classic"));
        assert_eq!(symbol_to_id("XLM"), Some("stellar"));
        assert_eq!(symbol_to_id("ATOM"), Some("cosmos"));
        assert_eq!(symbol_to_id("XMR"), Some("monero"));
        assert_eq!(symbol_to_id("PEPE"), Some("pepe"));
        assert_eq!(symbol_to_id("WIF"), Some("dogwifcoin"));
        assert_eq!(symbol_to_id("RENDER"), Some("render-token"));
    }

    #[test]
    fn suffix_combos_lowercase_and_mixed() {
        // USDT suffix
        assert_eq!(symbol_to_id("ARBUSDT"), Some("arbitrum"));
        assert_eq!(symbol_to_id("OPUSDT"), Some("optimism"));
        // USD suffix
        assert_eq!(symbol_to_id("SUIUSD"), Some("sui"));
        assert_eq!(symbol_to_id("ATOMUSD"), Some("cosmos"));
        // USDC suffix
        assert_eq!(symbol_to_id("INJUSDC"), Some("injective-protocol"));
        assert_eq!(symbol_to_id("AAVEUSDC"), Some("aave"));
        // lowercase / mixed case
        assert_eq!(symbol_to_id("pepeusdt"), Some("pepe"));
        assert_eq!(symbol_to_id("SeiUsd"), Some("sei-network"));
        assert_eq!(symbol_to_id("hbarusdc"), Some("hedera-hashgraph"));
    }

    #[test]
    fn render_alias() {
        // Both RENDER and the legacy RNDR ticker map to the same id.
        assert_eq!(symbol_to_id("RNDR"), Some("render-token"));
        assert_eq!(symbol_to_id("RENDER"), Some("render-token"));
    }

    #[test]
    fn still_unknown_returns_none() {
        assert_eq!(symbol_to_id("NOTACOIN"), None);
        assert_eq!(symbol_to_id("FOOUSDT"), None);
    }
}
