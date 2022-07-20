use serde::{Deserialize, Serialize};
use serde_json;
use std::error::Error;
use std::fs::File;
use std::io::BufReader;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CypherConfig {
    pub clusters: Clusters,
    pub groups: Vec<CypherGroupConfig>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Clusters {
    pub devnet: ClusterConfig,
    pub mainnet: ClusterConfig,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterConfig {
    pub rpc_url: String,
    pub pubsub_url: String,
}

impl CypherConfig {
    pub fn get_config_for_cluster(&self, cluster: &str) -> &ClusterConfig {
        match cluster {
            "devnet" => &self.clusters.devnet,
            "mainnet" => &self.clusters.mainnet,
            "" => &self.clusters.devnet,
            &_ => &self.clusters.devnet,
        }
    }

    pub fn get_group(&self, cluster: &str) -> Option<&CypherGroupConfig> {
        self.groups.iter().find(|&g| g.name.as_str() == cluster)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CypherGroupConfig {
    pub cluster: String,
    pub name: String,
    pub quote_symbol: String,
    pub address: String,
    pub program_id: String,
    pub serum_program_id: String,
    pub tokens: Vec<CypherTokenConfig>,
    pub oracles: Vec<CypherOracleConfig>,
    pub markets: Vec<CypherMarketConfig>,
}

impl CypherGroupConfig {
    pub fn get_market(&self, market: &str) -> Option<&CypherMarketConfig> {
        self.markets.iter().find(|&m| m.name.as_str() == market)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CypherTokenConfig {
    pub symbol: String,
    pub mint: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CypherOracleConfig {
    pub symbol: String,
    pub address: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CypherMarketConfig {
    pub name: String,
    pub base_symbol: String,
    pub quote_symbol: String,
    pub market_type: String,
    pub pair_base_symbol: String,
    pub pair_quote_symbol: String,
    pub address: String,
    pub base_decimals: u8,
    pub quote_decimals: u8,
    pub market_index: usize,
    pub bids: String,
    pub asks: String,
    pub event_queue: String,
}

pub fn load_cypher_config(path: &str) -> Result<CypherConfig, Box<dyn Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let cypher_config: CypherConfig = serde_json::from_reader(reader).unwrap();
    Ok(cypher_config)
}
