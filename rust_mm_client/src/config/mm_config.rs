use {
    crate::{
        market_maker::{InventoryManagerConfig, OrderManagerConfig}
    },
    serde::{Deserialize, Serialize},
    serde_json,
    std::{error::Error, fs::File, io::BufReader},
};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketMakerConfig {
    pub wallet: String,
    pub group: String,
    pub inventory_manager_config: InventoryManagerConfig,
    pub order_manager_config: OrderManagerConfig,
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
