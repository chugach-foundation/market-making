use serde::{Deserialize, Serialize};
use serde_json;
use std::error::Error;
use std::fs::File;
use std::io::BufReader;

use crate::market_maker::InventoryManagerConfig;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketMakerConfig {
    pub wallet: String,
    pub cluster: String,
    pub inventory_manager_config: InventoryManagerConfig,
    pub market: MarketConfig,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketConfig {
    pub name: String,
}

pub fn load_mm_config(path: &str) -> Result<MarketMakerConfig, Box<dyn Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mm_config: MarketMakerConfig = serde_json::from_reader(reader).unwrap();
    Ok(mm_config)
}
