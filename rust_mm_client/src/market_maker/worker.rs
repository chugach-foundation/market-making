use cypher::states::{CypherGroup, CypherMarket, CypherUser};
use log::{info, warn};
use serum_dex::state::MarketStateV2;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey, signature::Keypair};
use std::{sync::Arc, time::Duration};
use tokio::sync::{
    broadcast::{channel, Receiver, Sender},
    Mutex, RwLock,
};

use crate::MarketMakerError;

use super::{order_manager::OrderManager, InventoryManager};

pub struct WorkerConfig {
    pub market: Pubkey,
    pub c_asset_mint: Pubkey,
    pub market_index: usize,
    pub symbol: String,
}

impl WorkerConfig {
    pub fn default() -> Self {
        Self {
            market: Pubkey::default(),
            c_asset_mint: Pubkey::default(),
            market_index: usize::default(),
            symbol: "".to_string(),
        }
    }
}

pub struct Worker {
    config: WorkerConfig,
    inventory_manager: Arc<InventoryManager>,
    order_manager: Arc<OrderManager>,
    cypher_account_receiver: Mutex<Receiver<Box<CypherUser>>>,
    cypher_group_receiver: Mutex<Receiver<Box<CypherGroup>>>,
    shutdown: Arc<Sender<bool>>,
    latest_price: RwLock<u64>,
    cypher_user: RwLock<Option<CypherUser>>,
    cypher_group: RwLock<Option<CypherGroup>>,
    cypher_market: RwLock<CypherMarket>,
    cypher_user_pubkey: Pubkey,
    open_orders_pubkey: Pubkey,
    signer: Keypair,
}

#[allow(clippy::too_many_arguments)]
impl Worker {
    pub fn default() -> Self {
        Self {
            config: WorkerConfig::default(),
            inventory_manager: Arc::new(InventoryManager::default()),
            order_manager: Arc::new(OrderManager::default()),
            cypher_account_receiver: Mutex::new(channel::<Box<CypherUser>>(u16::MAX as usize).1),
            cypher_group_receiver: Mutex::new(channel::<Box<CypherGroup>>(u16::MAX as usize).1),
            shutdown: Arc::new(channel::<bool>(1).0),
            latest_price: RwLock::new(u64::default()),
            cypher_user: RwLock::new(None),
            cypher_group: RwLock::new(None),
            cypher_market: RwLock::new(CypherMarket::default()),
            cypher_user_pubkey: Pubkey::default(),
            open_orders_pubkey: Pubkey::default(),
            signer: Keypair::new(),
        }
    }

    pub fn new(
        config: WorkerConfig,
        inventory_manager: Arc<InventoryManager>,
        order_manager: Arc<OrderManager>,
        cypher_account_receiver: Receiver<Box<CypherUser>>,
        cypher_group_receiver: Receiver<Box<CypherGroup>>,
        shutdown: Arc<Sender<bool>>,
        cypher_user_pubkey: Pubkey,
        open_orders_pubkey: Pubkey,
    ) -> Self {
        Self {
            config,
            inventory_manager,
            order_manager,
            cypher_account_receiver: Mutex::new(cypher_account_receiver),
            cypher_group_receiver: Mutex::new(cypher_group_receiver),
            shutdown,
            cypher_user_pubkey,
            open_orders_pubkey,
            ..Worker::default()
        }
    }

    pub fn set_keypair(&mut self, keypair: Keypair) {
        self.signer = keypair;
    }

    pub async fn start(self) {
        let aself = Arc::new(self);

        let cp_cloned_self = Arc::clone(&aself);
        let cp_update_t = tokio::spawn(async move {
            let res = cp_cloned_self.process_provider_updates().await;

            match res {
                Ok(_) => (),
                Err(_) => {
                    warn!(
                        "[WORKER-{}-PROVIDERS] There was an error receiving a provider update.",
                        cp_cloned_self.config.symbol
                    );
                }
            }
        });

        let mut shutdown = aself.shutdown.subscribe();

        tokio::select! {
            res = aself.process() => {
                match res {
                    Ok(_) => (),
                    Err(e) => {
                        warn!(
                            "[WORKER-{}] An error occurred while running the worker: {:?}",
                            aself.config.symbol, e
                        );
                    }
                }
            },
            _ = shutdown.recv() => {
                info!(
                    "[WORKER-{}] Received shutdown signal, stopping.",
                    aself.config.symbol
                );
                match aself.shutdown().await {
                    Ok(_) => (),
                    Err(e) => {
                        warn!(
                            "[WORKER-{}] An error occurred while terminating the worker: {:?}",
                            aself.config.symbol, e
                        );
                    }
                };
            }
        }

        let (cp_res,) = tokio::join!(cp_update_t);
        match cp_res {
            Ok(_) => (),
            Err(_) => {
                warn!(
                    "[WORKER-{}] There was an error while joining with the worker providers task.",
                    aself.config.symbol
                );
            }
        };
    }

    async fn process(self: &Arc<Self>) -> Result<(), MarketMakerError> {
        loop {
            let maybe_user = self.cypher_user.read().await;
            if maybe_user.is_none() {
                continue;
            };
            let cypher_user = maybe_user.unwrap();

            let maybe_group = self.cypher_group.read().await;
            if maybe_group.is_none() {
                continue;
            };
            let cypher_group = maybe_group.unwrap();
            let cypher_token = cypher_group.get_cypher_token(self.config.market_index);

            let quote_vols = self
                .inventory_manager
                .get_quote_volumes(&cypher_user, &cypher_group, cypher_token)
                .await;

            info!(
                "[WORKER-{}] Current delta: {} | Desired Bid Size: {} | Desired Ask Size: {}.",
                self.config.symbol, quote_vols.delta, quote_vols.bid_size, quote_vols.ask_size
            );

            let latest_price = *self.latest_price.read().await;
            let (best_bid, best_ask) = self.inventory_manager.get_spread(latest_price);
            if best_ask == 0 || best_bid == 0 {
                continue;
            };

            info!(
                "[WORKER-{}] Desired spread: Bid: {} | Ask: {}.",
                self.config.symbol, best_bid, best_ask
            );

            let cypher_market = *self.cypher_market.read().await;

            info!("[WORKER-{}] Updating orders.", self.config.symbol);

            match self
                .order_manager
                .update_orders(
                    &cypher_group,
                    &cypher_market,
                    cypher_token,
                    &self.signer,
                    &quote_vols,
                    best_bid,
                    best_ask,
                )
                .await
            {
                Ok(_) => (),
                Err(e) => {
                    warn!(
                        "[WORKER-{}] An error occurred while updating orders: {:?}",
                        self.config.symbol, e
                    );
                }
            }

            tokio::time::sleep(Duration::from_millis(750)).await;
        }
    }

    async fn shutdown(self: &Arc<Self>) -> Result<(), MarketMakerError> {
        let maybe_user = self.cypher_user.read().await;
        if maybe_user.is_none() {
            return Ok(());
        };
        let cypher_user = maybe_user.unwrap();

        let maybe_group = self.cypher_group.read().await;
        if maybe_group.is_none() {
            return Ok(());
        };
        let cypher_group = maybe_group.unwrap();
        let cypher_token = cypher_group.get_cypher_token(self.config.market_index);

        let quote_vols = self
            .inventory_manager
            .get_quote_volumes(&cypher_user, &cypher_group, cypher_token)
            .await;

        info!(
            "[WORKER-{}] Current delta: {} | Desired Bid Size: {} | Desired Ask Size: {}.",
            self.config.symbol, quote_vols.delta, quote_vols.bid_size, quote_vols.ask_size
        );

        let latest_price = *self.latest_price.read().await;
        let (best_bid, best_ask) = self.inventory_manager.get_spread(latest_price);
        if best_ask == 0 || best_bid == 0 {
            return Ok(());
        };

        info!(
            "[WORKER-{}] Desired spread: Bid: {} | Ask: {}.",
            self.config.symbol, best_bid, best_ask
        );

        let cypher_market = *self.cypher_market.read().await;

        info!("[WORKER-{}] Updating orders.", self.config.symbol);

        match self
            .order_manager
            .cancel_orders_remain_neutral(
                &cypher_group,
                &cypher_market,
                cypher_token,
                &self.signer,
                &quote_vols,
                best_bid,
                best_ask,
            )
            .await
        {
            Ok(_) => (),
            Err(e) => {
                warn!(
                    "[WORKER-{}] An error occurred while updating orders: {:?}",
                    self.config.symbol, e
                );
            }
        }

        Ok(())
    }

    async fn process_provider_updates(self: &Arc<Self>) -> Result<(), MarketMakerError> {
        let mut group_receiver = self.cypher_group_receiver.lock().await;
        let mut account_receiver = self.cypher_account_receiver.lock().await;
        let mut shutdown = self.shutdown.subscribe();
        let mut shutdown_signal: bool = false;

        loop {
            tokio::select! {
                group = group_receiver.recv() => {
                    if group.is_err() {
                        warn!("[WORKER-{}] There was an error while processing cypher group updates, restarting loop.", self.config.symbol);
                        continue;
                    } else {
                        let cg = group.unwrap();
                        *self.cypher_group.write().await = Some(*cg);

                        let market_idx = cg.get_market_idx(self.config.c_asset_mint).unwrap();
                        let market = cg.get_cypher_market(market_idx);

                        info!(
                            "[WORKER-{}] Market update: Oracle price: {} - TWAP: {}",
                            self.config.symbol, &market.oracle_price.price, &market.market_price
                        );

                        *self.cypher_market.write().await = *market;
                        *self.latest_price.write().await = market.oracle_price.price;
                    }
                }
                account = account_receiver.recv() => {
                    if account.is_err() {
                        warn!("[WORKER-{}] There was an error while processing cypher account updates, restarting loop.", self.config.symbol);
                        continue;
                    } else {
                        *self.cypher_user.write().await = Some(*account.unwrap());
                    }
                }
                _ = shutdown.recv() => {
                    shutdown_signal = true;
                }
            }

            if shutdown_signal {
                warn!(
                    "[WORKER-{}] Received shutdown signal, stopping.",
                    self.config.symbol
                );
                break;
            }
        }

        Ok(())
    }
}
