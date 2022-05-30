
pub mod mm_client;
pub mod math;
pub mod accountinfoservice;
pub mod serumslab;
pub mod chainmetaservice;
pub mod fasttxnbuilder;

use std::{str::FromStr, sync::Arc};

pub use accountinfoservice::AccountInfoService;
use arrayref::array_refs;
use serum_dex::state::Market;
use solana_sdk::{pubkey::Pubkey, commitment_config::CommitmentConfig};
use solana_client::{nonblocking::rpc_client::RpcClient, client_error::ClientError};

use crate::serumslab::Slab;


async fn test_deserialize_orderbook(){
    let serumbidkey = Pubkey::from_str("14ivtgssEBoBjuZJtSAPKYgpUK7DmnSwuPMqJoVTSgKJ").unwrap();
    let serumaskkey = Pubkey::from_str("CEQdAFKdycHugujQg9k2wbmxjcpdYZyVLfV9WerTnafJ").unwrap();
    let keys = vec![serumbidkey, serumaskkey];
    let client = RpcClient::new_with_commitment("http://116.202.245.125:8899".to_string(), CommitmentConfig::processed());
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
    let book = bids.get_depth(5, 100, 100000000, false, 10_u64.pow(9u32), 10_u64.pow(6u32));
    println!("top p {}, q {}", book[0].price, book[0].quantity);
}

#[tokio::main]
async fn main() {
    println!("Hello, world!");
    let client = mm_client::MM_Client{};
    test_deserialize_orderbook().await;
}
