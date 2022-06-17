use std::{sync::Arc, time::Duration, num::NonZeroU64};
use cypher::states::{CypherUser, CypherGroup, CypherMarket, CypherToken};
use log::{warn, info};
use serum_dex::{state::{MarketStateV2, OpenOrders}, matching::{Side, OrderType}, instruction::{CancelOrderInstructionV2, NewOrderInstructionV3, SelfTradeBehavior}};
use solana_client::{client_error::ClientError, nonblocking::rpc_client::RpcClient, rpc_config::RpcTransactionConfig};
use solana_transaction_status::UiTransactionEncoding;
use solana_sdk::{pubkey::Pubkey, transaction::Transaction, instruction::Instruction, signature::Keypair, commitment_config::CommitmentConfig, hash::Hash};
use tokio::sync::{RwLock, broadcast::{Receiver, channel}, Mutex};

use crate::{
    providers::{
        orderbook_provider::OrderBook,
    },
    market_maker::{
        utils::get_open_orders, get_open_orders_with_qty
    },
    MarketMakerError,
    services::ChainMetaService,
    fast_tx_builder::FastTxnBuilder
};

use super::{InventoryManager, OpenOrder, QuoteVolumes, get_cancel_order_ix, get_new_order_ix};

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

#[derive(Debug, Default)]
pub struct InflightOrders {
    pub new_orders: RwLock<Vec<u64>>,
    pub cancelling_orders: RwLock<Vec<u64>>
}


pub struct Worker {
    config: WorkerConfig,
    rpc_client: Arc<RpcClient>,
    chain_meta_service: Arc<ChainMetaService>,
    inventory_manager: Arc<InventoryManager>,
    market_state: Option<MarketStateV2>,
    orderbook_receiver: Mutex<Receiver<Arc<OrderBook>>>,
    cypher_account_receiver: Mutex<Receiver<Box<CypherUser>>>,
    cypher_group_receiver: Mutex<Receiver<Box<CypherGroup>>>,
    open_orders_receiver: Mutex<Receiver<OpenOrders>>,
    shutdown: Receiver<bool>,
    latest_price: RwLock<u64>,
    orderbook: RwLock<Arc<OrderBook>>,
    cypher_user: RwLock<Option<CypherUser>>,
    cypher_group: RwLock<Option<CypherGroup>>,
    cypher_market: RwLock<CypherMarket>,
    open_orders: RwLock<Option<OpenOrders>>,
    cypher_user_pubkey: Pubkey,
    open_orders_pubkey: Pubkey,
    signer: Keypair,
    client_order_id: RwLock<u64>,
    inflight_orders: RwLock<InflightOrders>,
}

#[allow(clippy::too_many_arguments)]
impl Worker {
    pub fn default() -> Self {
        Self { 
            config: WorkerConfig::default(),
            rpc_client: Arc::new(RpcClient::new("http://localhost:8899".to_string())),
            chain_meta_service: Arc::new(ChainMetaService::default()),
            inventory_manager: Arc::new(InventoryManager::default()),
            market_state: None,
            orderbook_receiver: Mutex::new(channel::<Arc<OrderBook>>(u16::MAX as usize).1),
            cypher_account_receiver: Mutex::new(channel::<Box<CypherUser>>(u16::MAX as usize).1),
            cypher_group_receiver: Mutex::new(channel::<Box<CypherGroup>>(u16::MAX as usize).1),
            open_orders_receiver: Mutex::new(channel::<OpenOrders>(u16::MAX as usize).1),
            shutdown: channel::<bool>(1).1,
            latest_price: RwLock::new(u64::default()),
            orderbook: RwLock::new(Arc::new(OrderBook::default())),
            cypher_user: RwLock::new(None),
            cypher_group: RwLock::new(None),
            cypher_market: RwLock::new(CypherMarket::default()),
            open_orders: RwLock::new(None),
            cypher_user_pubkey: Pubkey::default(),
            open_orders_pubkey: Pubkey::default(),
            signer: Keypair::new(),
            client_order_id: RwLock::new(1_000_000_u64),
            inflight_orders: RwLock::new(InflightOrders::default()),
        }
    }

    pub fn new(
        config: WorkerConfig,
        rpc_client: Arc<RpcClient>,
        chain_meta_service: Arc<ChainMetaService>,
        inventory_manager: Arc<InventoryManager>,
        market_state: MarketStateV2,
        orderbook_receiver: Receiver<Arc<OrderBook>>,
        cypher_account_receiver: Receiver<Box<CypherUser>>,
        cypher_group_receiver: Receiver<Box<CypherGroup>>,
        open_orders_receiver: Receiver<OpenOrders>,
        shutdown: Receiver<bool>,
        cypher_user_pubkey: Pubkey,
        open_orders_pubkey: Pubkey,
    ) -> Self {
        Self {  
            config,
            rpc_client,
            chain_meta_service,
            inventory_manager,
            market_state: Some(market_state),
            orderbook_receiver: Mutex::new(orderbook_receiver),
            cypher_account_receiver: Mutex::new(cypher_account_receiver),
            cypher_group_receiver: Mutex::new(cypher_group_receiver),
            open_orders_receiver: Mutex::new(open_orders_receiver),
            shutdown,
            cypher_user_pubkey,
            open_orders_pubkey,
            ..Worker::default()
        }
    }

    pub fn set_keypair(
        &mut self,
        keypair: Keypair,
    ) {
        self.signer = keypair;
    }

    pub async fn start(
        self
    ) -> Result<(), MarketMakerError> {
        let aself = Arc::new(self);

        let cg_cloned_self = Arc::clone(&aself);
        let group_update_t = tokio::spawn(
            async move {
                let res = cg_cloned_self.start_process_group_updates().await;

                match res {
                    Ok(_) => (),
                    Err(_) => {
                        warn!("[WORKER-{}] There was an error receiving a group update.", cg_cloned_self.config.symbol);
                    } 
                };
            }
        );

        let ob_cloned_self = Arc::clone(&aself);
        let ob_update_t = tokio::spawn(
            async move {
                let res = ob_cloned_self.start_process_ob_updates().await;

                match res {
                    Ok(_) => (),
                    Err(_) => {
                        warn!("[WORKER-{}] There was an error receiving an orderbook update.", ob_cloned_self.config.symbol);
                    }
                }
            }
        );

        let oo_cloned_self = Arc::clone(&aself);
        let oo_update_t = tokio::spawn(
            async move {
                let res = oo_cloned_self.start_process_oo_updates().await;

                match res {
                    Ok(_) => (),
                    Err(_) => {
                        warn!("[WORKER-{}] There was an error receiving an open orders update.", oo_cloned_self.config.symbol);
                    }
                }
            }
        );

        let ca_cloned_self = Arc::clone(&aself);
        let ca_update_t = tokio::spawn(
            async move {
                let res = ca_cloned_self.start_process_ca_updates().await;

                match res {
                    Ok(_) => (),
                    Err(_) => {
                        warn!("[WORKER-{}] There was an error receiving a cypher account update.", ca_cloned_self.config.symbol);
                    }
                }
            }
        );

        let p_cloned_self = Arc::clone(&aself);
        let p_t = tokio::spawn(
            async move {
                let res = p_cloned_self.process().await;
                
                match res {
                    Ok(_) => (),
                    Err(_) => {
                        warn!("[WORKER-{}] There was an error during the worker process loop.", p_cloned_self.config.symbol);
                    }
                }
            }
        );

        let (g_update_res,
            ob_update_res,
            oo_update_res,
            ca_update_res,
            p_res) = tokio::join!(
            group_update_t,
            ob_update_t,
            oo_update_t,
            ca_update_t,
            p_t
        );

        match g_update_res {
            Ok(_) => (),
            Err(_) => {
                warn!("[WORKER-{}] There was an error while joining with the group update task.", aself.config.symbol);
                return Err(MarketMakerError::JoiningTaskError);
            }
        };

        match ob_update_res {
            Ok(_) => (),
            Err(_) => {
                warn!("[WORKER-{}] There was an error while joining with the orderbook update task.", aself.config.symbol);
                return Err(MarketMakerError::JoiningTaskError);
            }
        };
        
        match oo_update_res {
            Ok(_) => (),
            Err(_) => {
                warn!("[WORKER-{}] There was an error while joining with the open orders update task.", aself.config.symbol);
                return Err(MarketMakerError::JoiningTaskError);
            }
        };

        match ca_update_res {
            Ok(_) => (),
            Err(_) => {
                warn!("[WORKER-{}] There was an error while joining with the cypher account update task.", aself.config.symbol);
                return Err(MarketMakerError::JoiningTaskError);
            }
        };

        
        match p_res {
            Ok(_) => (),
            Err(_) => {
                warn!("[WORKER-{}] There was an error while joining with the worker proccess task.", aself.config.symbol);
                return Err(MarketMakerError::JoiningTaskError);
            }
        };

        Ok(())
    }
    
    async fn process(
        self: &Arc<Self>,
    ) -> Result<(), MarketMakerError>{        
        loop {            
            let maybe_user = self.cypher_user.read().await;
            if maybe_user.is_none() {
                tokio::time::sleep(Duration::from_millis(1000)).await;
                continue;
            };
            let cypher_user = maybe_user.unwrap();

            let maybe_group = self.cypher_group.read().await;
            if maybe_group.is_none() {
                tokio::time::sleep(Duration::from_millis(1000)).await;
                continue;
            };
            let cypher_group = maybe_group.unwrap();
            let maybe_oo = self.open_orders.read().await;
            if maybe_oo.is_none() {
                tokio::time::sleep(Duration::from_millis(1000)).await;
                continue;
            }
            let oo = maybe_oo.unwrap();
            let cypher_market = self.cypher_market.read().await;
            let rwb = self.orderbook.read().await;
            let latest_price = *self.latest_price.read().await;
            let book = rwb.as_ref();
            let inflight_orders = self.inflight_orders.read().await;
            let new_orders = inflight_orders.new_orders.read().await;

            let orders = get_open_orders_with_qty(&oo, book).await;
            info!("[WORKER-{}] Found {} orders resting.", self.config.symbol, orders.len());

            // first we check if we don't have any orders and if we already submitted orders
            if orders.is_empty() && !new_orders.is_empty() {
                info!("[WORKER-{}] Found {} inflight new orders, sleeping.", self.config.symbol, new_orders.len());
                tokio::time::sleep(Duration::from_millis(1000)).await;
                continue;
            }

            let mut new_orders_remove: Vec<usize> = Vec::new();
            // then we check if we have any in flight orders that we haven't removed yet
            for order in orders.iter() {
                let inflight_order_idx = new_orders.iter().position(|&o| o == order.client_order_id);

                if inflight_order_idx.is_some() {
                    new_orders_remove.push(inflight_order_idx.unwrap());
                }
            }
            drop(new_orders);
            let mut new_orders = inflight_orders.new_orders.write().await;
            for o in new_orders_remove {
                new_orders.remove(o);
            }
            drop(new_orders);

            let cypher_token = cypher_group.get_cypher_token(self.config.market_index);            

            info!("[WORKER-{}] Calculating quote volumes..", self.config.symbol);
            let quote_vols = self.inventory_manager.get_quote_volumes(
                &cypher_user,
                &cypher_group,
                cypher_token
            ).await;

            if quote_vols.ask_size == 0 || quote_vols.bid_size == 0 {
                info!("[WORKER-{}] Desired ask or quote vol is zero.", self.config.symbol);
                tokio::time::sleep(Duration::from_millis(1000)).await;
                continue;
            }

            info!("[WORKER-{}] Desired quote volumes: Bid: {} - Ask {}.", self.config.symbol, quote_vols.bid_size, quote_vols.ask_size);

            info!("[WORKER-{}] Calculating desired spread..", self.config.symbol);
            let (best_bid, best_ask) = self.inventory_manager.get_spread(latest_price);
            if best_ask == 0 || best_bid == 0 {
                tokio::time::sleep(Duration::from_millis(1000)).await;
                continue;
            };
            
            info!("[WORKER-{}] Desired spread: Bid: {} - Ask {}.", self.config.symbol, best_bid, best_ask);

            info!("[WORKER-{}] Getting stale orders..", self.config.symbol);
            let stale_orders = self.get_stale_orders(
                &orders,
                &quote_vols,
                best_bid,
                best_ask
            );
            info!("[WORKER-{}] Found {} stale orders.", self.config.symbol, stale_orders.len());

            let cancelling_orders = inflight_orders.cancelling_orders.write().await;
            // first we check if we have any stale orders and order cancels in flight
            if !stale_orders.is_empty() && !cancelling_orders.is_empty() {
                info!("[WORKER-{}] Found {} inflight orders being cancelled, sleeping.", self.config.symbol, cancelling_orders.len());
                tokio::time::sleep(Duration::from_millis(1500)).await;
                continue;
            }
            let mut cancel_orders_remove: Vec<usize> = Vec::new();
            // then we check if we have any in flight order cancels we haven't removed yet
            for order in stale_orders.iter() {
                let inflight_order_idx = cancelling_orders.iter().position(|&o| o == order.client_order_id);

                if inflight_order_idx.is_some() {
                    cancel_orders_remove.push(inflight_order_idx.unwrap());
                }
            }
            drop(cancelling_orders);
            let mut cancelling_orders = inflight_orders.cancelling_orders.write().await;
            for o in cancel_orders_remove {
                cancelling_orders.remove(o);
            }
            drop(cancelling_orders);
            drop(inflight_orders);
            
            info!("[WORKER-{}] Fetching orders to cancel or add.", self.config.symbol);

            let mut ixs: Vec<Instruction> = Vec::new();

            if !stale_orders.is_empty() {
                let cancel_ixs = self.get_cancel_orders_ixs(
                    &stale_orders,
                    &cypher_group,
                    &cypher_market,
                    cypher_token
                ).await;
                info!("[WORKER-{}] Cancelling {} stale orders.", self.config.symbol, cancel_ixs.len());
                ixs.extend(cancel_ixs);      
            }

            if orders.is_empty() || !stale_orders.is_empty(){
                let new_order_ixs = self.get_new_orders_ixs(
                    &cypher_group,
                    &cypher_market,
                    cypher_token,
                    &quote_vols,
                    best_bid,
                    best_ask,
                    latest_price
                ).await;
                info!("[WORKER-{}] Submitting {} new orders.", self.config.symbol, new_order_ixs.len());
                ixs.extend(new_order_ixs);
            }
            
            let blockhash = self.chain_meta_service.get_latest_blockhash().await;
            if blockhash == Hash::default() {
                tokio::time::sleep(Duration::from_millis(1000)).await;
            };

            info!("[WORKER-{}] Using blockhash {}", self.config.symbol, blockhash);

            let cloned_self = Arc::clone(self);
            tokio::spawn(
                async move {
                    let res = cloned_self.submit_transactions(ixs, blockhash).await;
                    match res {
                        Ok(_) => (),
                        Err(e) => {
                            warn!("Failed to submit transaction: {}", e.to_string());
                        },
                    }
                }
            );

            tokio::time::sleep(Duration::from_millis(1000)).await;
        }
    }

    async fn get_new_orders_ixs(
        self: &Arc<Self>,
        cypher_group: &CypherGroup,
        cypher_market: &CypherMarket,
        cypher_token: &CypherToken,
        quote_vols: &QuoteVolumes,
        best_bid: u64,
        best_ask: u64,
        latest_price: u64,
    )-> Vec<Instruction> {        
        let mut ixs: Vec<Instruction> = Vec::new();
        let inflight_orders = self.inflight_orders.write().await;
        let mut new_orders = inflight_orders.new_orders.write().await;

        let max_native_pc_qty_ask = quote_vols.ask_size as u64 * latest_price;
        
        info!("[WORKER-{}] Submitting new ask at {} for {} units at max qty pc {}", self.config.symbol, best_ask, quote_vols.ask_size, max_native_pc_qty_ask);
        ixs.push(
            get_new_order_ix(
                cypher_group,
                cypher_market,
                cypher_token,
                &self.market_state.unwrap(),
                &self.open_orders_pubkey,
                &self.cypher_user_pubkey,
                &self.signer,
                NewOrderInstructionV3 {
                    client_order_id: *self.client_order_id.read().await,
                    limit: u16::MAX,
                    limit_price: NonZeroU64::new(best_ask).unwrap(),
                    side: Side::Ask,
                    max_coin_qty: NonZeroU64::new(quote_vols.ask_size as u64).unwrap(),
                    max_native_pc_qty_including_fees: NonZeroU64::new(max_native_pc_qty_ask).unwrap(),
                    order_type: OrderType::Limit,
                    self_trade_behavior: SelfTradeBehavior::CancelProvide,
                }                
            )
        );
        *self.client_order_id.write().await += 1;
        new_orders.push(*self.client_order_id.read().await);

        let max_native_pc_qty_bid = quote_vols.bid_size as u64 * latest_price;

        info!("[WORKER-{}] Submitting new bid at {} for {} units at max pc qty {}", self.config.symbol, best_bid, quote_vols.bid_size, max_native_pc_qty_bid);
        ixs.push(
            get_new_order_ix(
                cypher_group,
                cypher_market,
                cypher_token,
                &self.market_state.unwrap(),
                &self.open_orders_pubkey,
                &self.cypher_user_pubkey,
                &self.signer,
                NewOrderInstructionV3 {
                    client_order_id: *self.client_order_id.read().await,
                    limit: u16::MAX,
                    limit_price: NonZeroU64::new(best_bid).unwrap(),
                    side: Side::Bid,
                    max_coin_qty: NonZeroU64::new(quote_vols.bid_size as u64).unwrap(),
                    max_native_pc_qty_including_fees: NonZeroU64::new(max_native_pc_qty_bid).unwrap(),
                    order_type: OrderType::Limit,
                    self_trade_behavior: SelfTradeBehavior::CancelProvide,
                }                
            )
        );
        *self.client_order_id.write().await += 1;
        new_orders.push(*self.client_order_id.read().await);

        ixs
    }

    async fn get_cancel_orders_ixs(
        self: &Arc<Self>,
        stale_orders: &Vec<OpenOrder>,
        cypher_group: &CypherGroup,
        cypher_market: &CypherMarket,
        cypher_token: &CypherToken,
    ) -> Vec<Instruction> {
        let inflight_orders = self.inflight_orders.write().await;
        let mut cancelling_orders = inflight_orders.cancelling_orders.write().await;
        let mut ixs: Vec<Instruction> = Vec::new();

        for order in stale_orders {
            info!("[WORKER-{}] Cancelling order with id {}", self.config.symbol, order.order_id);
            ixs.push(get_cancel_order_ix(
                cypher_group,
                cypher_market,
                cypher_token,
                &self.market_state.unwrap(),
                &self.open_orders_pubkey,
                &self.cypher_user_pubkey,
                &self.signer,
                CancelOrderInstructionV2{
                    order_id: order.order_id,
                    side: order.side
                }
            ));
            cancelling_orders.push(order.client_order_id);
        }

        ixs
    }

    fn get_stale_orders(
        self: &Arc<Self>,
        open_orders: &Vec<OpenOrder>,
        quote_vols: &QuoteVolumes,
        best_bid: u64,
        best_ask: u64,
    ) -> Vec<OpenOrder> {
        let mut so: Vec<OpenOrder> = Vec::new();

        for order in open_orders {
            if order.side == Side::Ask && (order.price != best_ask || order.quantity != quote_vols.ask_size as u64) {
                so.push(OpenOrder {
                    order_id: order.order_id,
                    client_order_id: order.client_order_id,
                    price: order.price,
                    quantity: order.quantity,
                    side: Side::Ask
                });

            } else if order.side == Side::Bid && (order.price != best_bid || order.quantity != quote_vols.bid_size as u64) {
                so.push(OpenOrder {
                    order_id: order.order_id,
                    client_order_id: order.client_order_id,
                    price: order.price,
                    quantity: order.quantity,
                    side: Side::Bid
                });
            }
        }

        so
    }

    async fn submit_transactions(
        self: &Arc<Self>,
        ixs: Vec<Instruction>,
        blockhash: Hash,
    ) -> Result<(), ClientError> {
        let mut txn_builder = FastTxnBuilder::new();
        let mut submitted: bool = false;
        let mut prev_tx: Transaction = Transaction::default();

        for ix in ixs {

            let tx = txn_builder.build(blockhash, &self.signer, None);
            // we do this to attempt to pack as many ixs in a tx as possible
            // there's more efficient ways to do it but we'll do it in the future
            if tx.message_data().len() > 1000 {
                let res = self.send_and_confirm_transaction(&prev_tx).await;
                submitted = true;
                match res {
                    Ok(_) => (),
                    Err(e) => {
                        warn!("There was an error submitting transaction and waiting for confirmation: {}", e.to_string());
                        return Err(e);
                    }
                }
            } else {
                txn_builder.add(ix);
                prev_tx = tx;
            }
        }

        if !submitted {
            let tx = txn_builder.build(blockhash, &self.signer, None);
            let res = self.send_and_confirm_transaction(&tx).await;
            match res {
                Ok(_) => (),
                Err(e) => {
                    warn!("There was an error submitting transaction and waiting for confirmation: {}", e.to_string());
                    return Err(e);
                }
            }
        }

        Ok(())
    }

    async fn send_and_confirm_transaction(
        self: &Arc<Self>,
        tx: &Transaction
    ) -> Result<(), ClientError> {
        let submit_res = self.rpc_client.send_and_confirm_transaction_with_spinner_and_commitment(
            tx,
            CommitmentConfig::confirmed()
        ).await;

        let signature = match submit_res {
            Ok(s) => {
                info!("Successfully submitted transaction. Transaction signature: {}", s.to_string());
                s
            },
            Err(e) => {
                warn!("There was an error submitting transaction: {}", e.to_string());
                return Err(e);
            }
        };

        loop {
            let confirmed = self.rpc_client.get_transaction_with_config(
                &signature,
                RpcTransactionConfig {
                    commitment: Some(CommitmentConfig::confirmed()),
                    encoding: Some(UiTransactionEncoding::Json),
                    max_supported_transaction_version: Some(0)
                }
            ).await;

            if confirmed.is_err() {
                tokio::time::sleep(Duration::from_millis(500)).await;
            } else {
                break;
            }
        }


        Ok(())
    }

    /// start a loop to process messages from the cypher group provider
    async fn start_process_group_updates(
        self: &Arc<Self>,
    ) -> Result<(), MarketMakerError> {
        loop {
            match self._process_group_updates().await {
                Ok(_) => {
                    info!("[WORKER-{}] Cypher group successfully updated, restarting loop.", self.config.symbol);
                },
                Err(_) => {
                    warn!("[WORKER-{}] There was an error while processing cypher group updates, restarting loop.", self.config.symbol);
                },
            };
        }
    }

    /// proccess cypher group updates
    async fn _process_group_updates(
        self: &Arc<Self>,
    ) -> Result<(), MarketMakerError> {
        let mut receiver = self.cypher_group_receiver.lock().await;

        if let Ok(group) = receiver.recv().await {
            *self.cypher_group.write().await = Some(*group);

            let market_idx = group.get_market_idx(self.config.c_asset_mint).unwrap();
            let market = group.get_cypher_market(market_idx);

            info!("[WORKER-{}] Market update: Oracle price: {} - TWAP: {}",
                self.config.symbol,
                &market.oracle_price.price,
                &market.market_price
            );
            
            *self.cypher_market.write().await = *market;
            *self.latest_price.write().await = market.oracle_price.price;
        }

        Ok(())
    }

    /// start a loop to process messages from the orderbook provider
    async fn start_process_ob_updates(
        self: &Arc<Self>,
    ) -> Result<(), MarketMakerError> {
        loop {
            match self._process_ob_updates().await {
                Ok(_) => {
                    info!("[WORKER-{}] Order book successfully updated, restarting loop.", self.config.symbol);
                },
                Err(_) => {
                    warn!("[WORKER-{}] There was an error while processing order book updates, restarting loop.", self.config.symbol);
                },
            };
        }
    }

    /// proccess order book updates
    async fn _process_ob_updates(
        self: &Arc<Self>,
    ) -> Result<(), MarketMakerError> {
        let mut receiver =  self.orderbook_receiver.lock().await;

        if let Ok(ob) = receiver.recv().await {
            let bids = ob.bids.read().await;
            let asks = ob.asks.read().await;
            if asks.is_empty() && !bids.is_empty() {
                info!("[WORKER-{}] Latest ob for market: {} bids / best bid {}@{} - 0 asks ",
                    self.config.symbol,
                    bids.len(),
                    bids[0].quantity,
                    bids[0].price,
                );
            } else if bids.is_empty() && !asks.is_empty() {
                info!("[WORKER-{}] Latest ob for market: 0 bids - {} asks / best ask {}@{}",
                    self.config.symbol,
                    asks.len(),
                    asks[0].quantity,
                    asks[0].price,
                );
            } else {
                info!("[WORKER-{}] Latest ob for market: {} bids / best bid {}@{} - {} asks / best ask {}@{}",
                    self.config.symbol,
                    bids.len(),
                    bids[0].quantity,
                    bids[0].price,
                    asks.len(),
                    asks[0].quantity,
                    asks[0].price,
                );
            }
            drop(bids);
            drop(asks);
            *self.orderbook.write().await = ob;
        }

        Ok(())
    }

    /// start a loop to process messages from the open orders provider
    async fn start_process_oo_updates(
        self: &Arc<Self>,
    ) -> Result<(), MarketMakerError> {
        loop {
            match self._process_oo_updates().await {
                Ok(_) => {
                    info!("[WORKER-{}] Open orders successfully updated, restarting loop.", self.config.symbol);
                },
                Err(_) => {
                    warn!("[WORKER-{}] There was an error while processing open orders updates, restarting loop.", self.config.symbol);
                },
            };
        }
    }

    /// proccess open orders updates
    async fn _process_oo_updates(
        self: &Arc<Self>,
    ) -> Result<(), MarketMakerError> {
        let mut receiver =  self.open_orders_receiver.lock().await;

        if let Ok(oo) = receiver.recv().await {
            *self.open_orders.write().await = Some(oo);
            let orders = get_open_orders(&oo);
            info!("[WORKER-{}] Open orders updated: {} orders resting", self.config.symbol, orders.len());            
        }

        Ok(())
    }

    
    /// start a loop to process messages from the cypher account provider
    async fn start_process_ca_updates(
        self: &Arc<Self>,
    ) -> Result<(), MarketMakerError> {
        loop {
            match self._process_ca_updates().await {
                Ok(_) => {
                    info!("[WORKER-{}] Cypher account successfully updated, restarting loop.", self.config.symbol);
                },
                Err(_) => {
                    warn!("[WORKER-{}] There was an error while processing cypher account updates, restarting loop.", self.config.symbol);
                },
            };
        }
    }

    /// proccess cypher account updates
    async fn _process_ca_updates(
        self: &Arc<Self>,
    ) -> Result<(), MarketMakerError> {
        let mut receiver =  self.cypher_account_receiver.lock().await;

        if let Ok(ca) = receiver.recv().await {
            *self.cypher_user.write().await = Some(*ca);
        }

        Ok(())
    }

}