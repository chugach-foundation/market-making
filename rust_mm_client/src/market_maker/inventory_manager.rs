use crate::config::MarketMakerConfig;
use cypher::states::{CypherGroup, CypherToken, CypherUser};
use jet_proto_math::Number;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryManagerConfig {
    pub initial_capital: u64,
    pub max_quote: i64,
    pub shape_num: u32,
    pub shape_denom: u32,
    pub spread: u8,
}

pub struct InventoryManager {
    config: Arc<MarketMakerConfig>,
    market_idx: usize,
    max_quote: i64,
    shape_num: u32,
    shape_denom: u32,
    spread: u8,
}

#[derive(Debug, Default)]
pub struct QuoteVolumes {
    pub delta: i64,
    pub bid_size: i64,
    pub ask_size: i64,
}

// Number we use here is arbitrary, shape mul can do conversion to any base..
const EXP_BASE: i64 = 2;
const BPS_UNIT: u64 = 10000;

impl InventoryManager {
    pub fn default() -> Self {
        Self {
            config: Arc::new(MarketMakerConfig::default()),
            market_idx: usize::default(),
            max_quote: i64::default(),
            shape_num: u32::default(),
            shape_denom: u32::default(),
            spread: u8::default(),
        }
    }

    pub fn new(
        config: Arc<MarketMakerConfig>,
        market_index: usize,
        max_quote: i64,
        shape_num: u32,
        shape_denom: u32,
        spread: u8,
    ) -> Self {
        Self {
            config,
            market_idx: market_index,
            max_quote,
            shape_num,
            shape_denom,
            spread,
        }
    }

    pub async fn get_quote_volumes(
        &self,
        user: &CypherUser,
        group: &CypherGroup,
        cypher_token: &CypherToken,
    ) -> QuoteVolumes {
        let current_delta = self.get_user_delta(user, group, cypher_token);

        let adjusted_vol = self.adj_quote_size(current_delta.abs().try_into().unwrap());
        let (bid_size, ask_size) = if current_delta < 0 {
            (self.max_quote, adjusted_vol)
        } else {
            (adjusted_vol, self.max_quote)
        };
        QuoteVolumes {
            delta: current_delta,
            bid_size,
            ask_size,
        }
    }

    fn get_user_delta(
        &self,
        cypher_user: &CypherUser,
        cypher_group: &CypherGroup,
        cypher_token: &CypherToken,
    ) -> i64 {
        let user_pos = cypher_user.get_position(self.market_idx).unwrap();

        info!(
            "[INVMGR-{}] Base Borrows: {}. Base Deposits: {}",
            self.config.market.name,
            user_pos.base_borrows(),
            user_pos.base_deposits(),
        );

        info!(
            "[INVMGR-{}] Native Borrows: {}. Native Deposits: {}",
            self.config.market.name,
            user_pos.native_borrows(cypher_token),
            user_pos.native_deposits(cypher_token),
        );

        info!(
            "[INVMGR-{}] Total Borrows: {}. Total Deposits: {}",
            self.config.market.name,
            user_pos.total_borrows(cypher_token),
            user_pos.total_deposits(cypher_token),
        );

        let long_pos = user_pos.total_deposits(cypher_token).as_u64(0);
        let short_pos = user_pos.total_borrows(cypher_token).as_u64(0);

        let delta = long_pos as i64 - short_pos as i64;

        info!(
            "[INVMGR-{}] Open Orders Coin Free: {}. Open Orders Coin Total: {}.",
            self.config.market.name, user_pos.oo_info.coin_free, user_pos.oo_info.coin_total,
        );

        info!(
            "[INVMGR-{}] Open Orders Price Coin Free: {}. Open Orders Price Coin Total: {}.",
            self.config.market.name, user_pos.oo_info.pc_free, user_pos.oo_info.pc_total,
        );

        let div: Number = 10_u64.checked_pow(6).unwrap().into();
        let assets_val = cypher_user.get_assets_value(cypher_group).unwrap();
        let assets_val_ui = assets_val / div;
        let liabs_val = cypher_user.get_liabs_value(cypher_group).unwrap();
        let liabs_val_ui = liabs_val / div;

        info!(
            "[INVMGR-{}] Assets value: {} - Liabilities value: {} ",
            self.config.market.name, assets_val_ui, liabs_val_ui
        );

        delta
    }

    fn adj_quote_size(&self, abs_delta: u32) -> i64 {
        let shaped_delta = self.shape_num * abs_delta;
        let divided_shaped_delta = shaped_delta / self.shape_denom;
        let divisor = EXP_BASE.pow(divided_shaped_delta);
        self.max_quote / divisor
    }

    pub fn get_spread(&self, oracle_price: u64) -> (u64, u64) {
        let num = (BPS_UNIT + self.spread as u64) as f64 / BPS_UNIT as f64;
        let best_ask = oracle_price as f64 * num;
        let best_bid = oracle_price as f64 / num;

        (best_bid as u64, best_ask as u64)
    }
}
