use arrayref::array_refs;
use tokio::sync::{
    broadcast::{Sender, Receiver, channel},
    Mutex
};
use solana_sdk::pubkey::Pubkey;
use tokio::sync::RwLock;
use std::sync::Arc;
use log::{info, warn};
use crate::{
    accounts_cache::AccountsCache,
    serum_slab::{Slab, OrderBookOrder}
};

#[derive(Default)]
pub struct OrderBook {
    pub market: Pubkey,
    pub bids: RwLock<Vec<OrderBookOrder>>,
    pub asks: RwLock<Vec<OrderBookOrder>>
}

impl OrderBook {
    pub fn new(market: Pubkey) -> Self {
        Self {
            market,
            bids: RwLock::new(Vec::new()),
            asks: RwLock::new(Vec::new())
        }
    }
}

pub struct OrderBookProvider {
    cache: Arc<AccountsCache>,
    sender: Arc<Sender<Arc<OrderBook>>>,
    receiver: Mutex<Receiver<Pubkey>>,
    book: Arc<OrderBook>,
    market: Pubkey,
    bids: Pubkey,
    asks: Pubkey,
    coin_lot_size: u64,
    pc_lot_size: u64,
    coin_decimals: u64,
}

impl OrderBookProvider {
    pub fn default() -> Self {
        Self { 
            cache: Arc::new(AccountsCache::default()), 
            sender: Arc::new(channel::<Arc<OrderBook>>(u16::MAX as usize).0), 
            receiver: Mutex::new(channel::<Pubkey>(u16::MAX as usize).1), 
            book: Arc::new(OrderBook::default()), 
            market: Pubkey::default(), 
            bids: Pubkey::default(), 
            asks: Pubkey::default(), 
            coin_lot_size: u64::default(), 
            pc_lot_size: u64::default(), 
            coin_decimals: u64::default(), 
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cache: Arc<AccountsCache>,
        sender: Arc<Sender<Arc<OrderBook>>>,
        receiver: Receiver<Pubkey>,
        market: Pubkey,
        bids: Pubkey,
        asks: Pubkey,
        coin_lot_size: u64,
        pc_lot_size: u64,
        coin_decimals: u64,
    ) -> Self {
        Self { 
            cache,
            sender,
            receiver: Mutex::new(receiver),
            book: Arc::new(OrderBook::new(market)),
            market,
            bids,
            asks,
            coin_lot_size,
            pc_lot_size,
            coin_decimals
        }
    }

    pub async fn start(
        self: &Arc<Self>
    ) {
        loop {            
            match self.process_updates().await {
                Ok(_) => {
                    //info!("[OBP] Orderbook provider successfully processed update, restarting loop.");
                },
                Err(e) => {
                    if e == OrderBookProviderError::ChannelSendError {                            
                        warn!("[OBP] There was an error while processing updates, restarting loop.");
                    }
                },
            };
        }
    }

    #[allow(clippy::ptr_offset_with_cast)]
    async fn process_updates(
        self: &Arc<Self>
    ) -> Result<(), OrderBookProviderError> {
        let mut receiver = self.receiver.lock().await;

        if let Ok(key) = receiver.recv().await {
            //info!("[OBP] Received account update with key: {}", key);
            let mut updated: bool = false;

            if key == self.bids {
                let bid_ai = self.cache.get(&key).unwrap();

                let (_bid_head, bid_data, _bid_tail) = array_refs![&bid_ai.account.data, 5; ..; 7];
                let bid_data = &mut bid_data[8..].to_vec().clone();
                let bids = Slab::new(bid_data);
                
                let obl = bids.get_depth(
                    25, 
                    self.pc_lot_size, 
                    self.coin_lot_size,
                    false,
                );
                
                *self.book.bids.write().await = obl;
                updated = true;

            } else if key == self.asks {
                let ask_ai = self.cache.get(&key).unwrap();

                let (_ask_head, ask_data, _ask_tail) = array_refs![&ask_ai.account.data, 5; ..; 7];
                let ask_data = &mut ask_data[8..].to_vec().clone();
                let asks = Slab::new(ask_data);

                let obl = asks.get_depth(
                    25, 
                    self.pc_lot_size, 
                    self.coin_lot_size,
                    true,
                );

                *self.book.asks.write().await = obl;
                updated = true;
            }

            if updated {
                let res = self.sender.send(Arc::clone(&self.book));

                match res {
                    Ok(_) => {
                        info!("[OBP] Updated orderbook for {}", self.market);
                    },
                    Err(_) => {
                        warn!("[OBP] There was an error sending an update about the orderbook for {}", self.market);
                        return Err(OrderBookProviderError::ChannelSendError);
                    },
                };
            }
        }

        Ok(())
    }

    pub fn get_order_book(&self, key: Pubkey) -> Arc<OrderBook> {
        info!("Fetching order book for market {}", &key.to_string());
        Arc::clone(&self.book)
    }

}

#[derive(Debug, PartialEq)]
pub enum OrderBookProviderError {
    ChannelSendError
}

// async fn test_deserialize_orderbook(client: Arc<RpcClient>, pubsub: Arc<PubsubClient>) {
//     let serumbidkey = Pubkey::from_str("14ivtgssEBoBjuZJtSAPKYgpUK7DmnSwuPMqJoVTSgKJ").unwrap();
//     let serumaskkey = Pubkey::from_str("CEQdAFKdycHugujQg9k2wbmxjcpdYZyVLfV9WerTnafJ").unwrap();
//     let keys = vec![serumbidkey, serumaskkey];
//     let service = Arc::new(AccountInfoService::new(
//         client,
//         pubsub,
//         &keys[..],
//         Vec::new().as_ref()
//     ));
//     service.start_service().await;
//     let map = service.get_account_map_read_lock().await;
//     let ac = map.get(&serumbidkey).unwrap();
//     let (_bid_head, bid_data, _bid_tail) = array_refs![&ac.data, 5; ..; 7];
//     let bid_data = &mut bid_data[8..].to_vec().clone();
//     let bids = Slab::new(bid_data);
//     let top = bids.remove_max().unwrap();
//     println!("hhh {}, {}", top.quantity(), top.price());
//     let ac2 = map.get(&serumaskkey).unwrap();
//     let (_ask_head, ask_data, _ask_tail) = array_refs![&ac.data, 5; ..; 7];
//     let ask_data = &mut ask_data[8..].to_vec().clone();
//     let asks = Slab::new(ask_data);
//     let top2 = asks.remove_min().unwrap();
//     println!("hhh {}, {}", top2.quantity(), top2.price());
//     //Gets the top 5 bids
//     let book = bids.get_depth(5, 100, 100000000, false, 10_u64.pow(9u32));
//     println!("top p {}, q {}", book[0].price, book[0].quantity);
// }