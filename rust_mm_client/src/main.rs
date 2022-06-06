pub mod account_info_service;
pub mod chain_meta_service;
pub mod cypher_group;
pub mod cypher_user;
pub mod fast_tx_builder;
pub mod math;
pub mod serum_slab;

use std::{str::FromStr, sync::Arc};

pub use account_info_service::AccountInfoService;
use arrayref::array_refs;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};

use crate::serum_slab::Slab;

async fn test_deserialize_orderbook() {
    let serumbidkey = Pubkey::from_str("14ivtgssEBoBjuZJtSAPKYgpUK7DmnSwuPMqJoVTSgKJ").unwrap();
    let serumaskkey = Pubkey::from_str("CEQdAFKdycHugujQg9k2wbmxjcpdYZyVLfV9WerTnafJ").unwrap();
    let keys = vec![serumbidkey, serumaskkey];
    let client = RpcClient::new_with_commitment(
        "http://116.202.245.125:8899".to_string(),
        CommitmentConfig::processed(),
    );
    let service = Arc::new(AccountInfoService::new(Arc::new(client), &keys[..]));
    service.start_service().await;
    let map = service.get_account_map_read_lock().await;
    let ac = map.get(&serumbidkey).unwrap();
    let (_bid_head, bid_data, _bid_tail) = array_refs![&ac.data, 5; ..; 7];
    let bid_data = &mut bid_data[8..].to_vec().clone();
    let bids = Slab::new(bid_data);
    let top = bids.remove_max().unwrap();
    println!("hhh {}, {}", top.quantity(), top.price());
    let ac2 = map.get(&serumaskkey).unwrap();
    let (_ask_head, ask_data, _ask_tail) = array_refs![&ac.data, 5; ..; 7];
    let ask_data = &mut ask_data[8..].to_vec().clone();
    let asks = Slab::new(ask_data);
    let top2 = asks.remove_min().unwrap();
    println!("hhh {}, {}", top2.quantity(), top2.price());
    //Gets the top 5 bids
    let book = bids.get_depth(5, 100, 100000000, false, 10_u64.pow(9u32));
    println!("top p {}, q {}", book[0].price, book[0].quantity);
}

#[tokio::main]
async fn main() {
    test_deserialize_orderbook().await;
}
