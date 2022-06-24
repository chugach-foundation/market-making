use crate::{
    accounts_cache::AccountsCache,
    serum_slab::{OrderBookOrder, Slab},
    MarketMakerError,
};
use arrayref::array_refs;
use log::{info, warn};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::{
    broadcast::{channel, Receiver, Sender},
    Mutex,
};

#[derive(Default)]
pub struct OrderBook {
    pub market: Pubkey,
    pub bids: RwLock<Vec<OrderBookOrder>>,
    pub asks: RwLock<Vec<OrderBookOrder>>,
}

impl OrderBook {
    pub fn new(market: Pubkey) -> Self {
        Self {
            market,
            bids: RwLock::new(Vec::new()),
            asks: RwLock::new(Vec::new()),
        }
    }
}

pub struct OrderBookProvider {
    cache: Arc<AccountsCache>,
    sender: Arc<Sender<Arc<OrderBook>>>,
    receiver: Mutex<Receiver<Pubkey>>,
    shutdown_receiver: Mutex<Receiver<bool>>,
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
            shutdown_receiver: Mutex::new(channel::<bool>(1).1),
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
        shutdown_receiver: Receiver<bool>,
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
            shutdown_receiver: Mutex::new(shutdown_receiver),
            book: Arc::new(OrderBook::new(market)),
            market,
            bids,
            asks,
            coin_lot_size,
            pc_lot_size,
            coin_decimals,
        }
    }

    pub async fn start(self: &Arc<Self>) {
        let mut receiver = self.receiver.lock().await;
        let mut shutdown = self.shutdown_receiver.lock().await;
        let mut shutdown_signal: bool = false;

        loop {
            tokio::select! {
                key = receiver.recv() => {
                    if key.is_err() {
                        warn!("[OBP] There was an error while processing a provider update, restarting loop.");
                        continue;
                    } else {
                        let res = self.process_updates(key.unwrap()).await;
                        match res {
                            Ok(_) => (),
                            Err(_) => {
                                warn!(
                                    "[OBP] There was an error sending an update about the orderbook for market: {}.",
                                    self.market
                                );
                            },
                        }
                    }
                },
                _ = shutdown.recv() => {
                    shutdown_signal = true;
                }
            }

            if shutdown_signal {
                info!("[OBP] Received shutdown signal, stopping.",);
                break;
            }
        }
    }

    #[allow(clippy::ptr_offset_with_cast)]
    async fn process_updates(self: &Arc<Self>, key: Pubkey) -> Result<(), MarketMakerError> {
        let mut updated: bool = false;

        if key == self.bids {
            let bid_ai = self.cache.get(&key).unwrap();

            let (_bid_head, bid_data, _bid_tail) = array_refs![&bid_ai.account.data, 5; ..; 7];
            let bid_data = &mut bid_data[8..].to_vec().clone();
            let bids = Slab::new(bid_data);

            let obl = bids.get_depth(25, self.pc_lot_size, self.coin_lot_size, false);

            *self.book.bids.write().await = obl;
            updated = true;
        } else if key == self.asks {
            let ask_ai = self.cache.get(&key).unwrap();

            let (_ask_head, ask_data, _ask_tail) = array_refs![&ask_ai.account.data, 5; ..; 7];
            let ask_data = &mut ask_data[8..].to_vec().clone();
            let asks = Slab::new(ask_data);

            let obl = asks.get_depth(25, self.pc_lot_size, self.coin_lot_size, true);

            *self.book.asks.write().await = obl;
            updated = true;
        }

        if updated {
            let res = self.sender.send(Arc::clone(&self.book));

            match res {
                Ok(_) => {
                    info!("[OBP] Updated orderbook for market: {}.", self.market);
                }
                Err(_) => {
                    return Err(MarketMakerError::ChannelSendError);
                }
            };
        }

        Ok(())
    }
}
