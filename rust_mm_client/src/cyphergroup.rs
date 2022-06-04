
use anchor_client::solana_client::nonblocking::rpc_client::RpcClient;
use cypher::{
    states::{CypherGroup,
    },
};
use cypher_tester::{parse_dex_account};
use serum_dex::{
    state::{MarketStateV2 as DexMarketStateV2},
};
use solana_sdk::{
    pubkey::Pubkey,
};
pub struct CypherGroupInfo {
    pub address: Pubkey,
    pub account_state: Box<CypherGroup>,
}

impl CypherGroupInfo {
    pub async fn load_dex_market(
        &self,
        market_idx: usize,
        rpc : &RpcClient
    ) -> DexMarketStateV2 {
        let cypher_market = self.account_state.get_cypher_market(market_idx);
        let dex_market_acc = rpc
            .get_account(&cypher_market.dex_market)
            .await
            .unwrap();
        parse_dex_account(dex_market_acc.data)
    }
}
