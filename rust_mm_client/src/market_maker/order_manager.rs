use {
    super::QuoteVolumes,
    crate::{
        fast_tx_builder::FastTxnBuilder,
        market_maker::{get_cancel_order_ix, get_new_order_ix},
        providers::OrderBook,
        serum_slab::OrderBookOrder,
        services::ChainMetaService,
        MarketMakerError,
    },
    cypher::{
        utils::{derive_cypher_user_address, derive_open_orders_address},
        CypherGroup, CypherMarket, CypherToken,
    },
    log::{info, warn},
    serde::{Deserialize, Serialize},
    serum_dex::{
        instruction::{CancelOrderInstructionV2, NewOrderInstructionV3, SelfTradeBehavior},
        matching::{OrderType, Side},
        state::{MarketStateV2, OpenOrders},
    },
    solana_client::{client_error::ClientError, nonblocking::rpc_client::RpcClient},
    solana_sdk::{
        hash::Hash,
        instruction::Instruction,
        pubkey::Pubkey,
        signature::{Keypair, Signature},
        signer::Signer,
        transaction::Transaction,
    },
    std::{num::NonZeroU64, sync::Arc},
    tokio::sync::{
        broadcast::{channel, Receiver},
        Mutex, RwLock,
    },
};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderManagerConfig {
    pub layers: u8,
    pub spacing_bps: u8,
    pub step_amount: u32,
}

// for level in range(0, self._buy_levels):
//     price = self.get_price() * (Decimal("1") - self._bid_spread - (level * self._order_level_spread))
//     price = market.quantize_order_price(self.trading_pair, price)
//     size = self._order_amount + (self._order_level_amount * level)
//     size = market.quantize_order_amount(self.trading_pair, size)
//     if size > 0:
//         buys.append(PriceSize(price, size))
// for level in range(0, self._sell_levels):
//     price = self.get_price() * (Decimal("1") + self._ask_spread + (level * self._order_level_spread))
//     price = market.quantize_order_price(self.trading_pair, price)
//     size = self._order_amount + (self._order_level_amount * level)
//     size = market.quantize_order_amount(self.trading_pair, size)
//     if size > 0:
//         sells.append(PriceSize(price, size))

pub struct ManagedOrder {
    pub order_id: u128,
    pub client_order_id: u64,
    pub price: u64,
    pub quantity: u64,
    pub side: Side,
}

#[derive(Debug, Default)]
pub struct InflightOrders {
    pub new_orders: RwLock<Vec<u64>>,
    pub cancelling_orders: RwLock<Vec<u64>>,
}

pub struct OrderManager {
    symbol: String,
    rpc_client: Arc<RpcClient>,
    chain_meta_service: Arc<ChainMetaService>,
    oo_receiver: Mutex<Receiver<OpenOrders>>,
    ob_receiver: Mutex<Receiver<Arc<OrderBook>>>,
    shutdown_receiver: Mutex<Receiver<bool>>,
    market_state: Option<MarketStateV2>,
    open_orders: RwLock<Option<OpenOrders>>,
    orderbook: RwLock<Arc<OrderBook>>,
    inflight_orders: RwLock<InflightOrders>,
    client_order_id: RwLock<u64>,
    signer: Arc<Keypair>,
    cypher_user_pubkey: Pubkey,
    open_orders_pubkey: Pubkey
}

impl OrderManager {
    pub fn default() -> Self {
        Self {
            symbol: "".to_string(),
            rpc_client: Arc::new(RpcClient::new("http://localhost:8899".to_string())),
            chain_meta_service: Arc::new(ChainMetaService::default()),
            oo_receiver: Mutex::new(channel::<OpenOrders>(u16::MAX as usize).1),
            ob_receiver: Mutex::new(channel::<Arc<OrderBook>>(u16::MAX as usize).1),
            shutdown_receiver: Mutex::new(channel::<bool>(1).1),
            market_state: None,
            open_orders: RwLock::new(None),
            orderbook: RwLock::new(Arc::new(OrderBook::default())),
            inflight_orders: RwLock::new(InflightOrders::default()),
            client_order_id: RwLock::new(1_u64),
            signer: Arc::new(Keypair::new()),
            cypher_user_pubkey: Pubkey::default(),
            open_orders_pubkey: Pubkey::default(),
        }
    }

    pub fn new(
        symbol: String,
        rpc_client: Arc<RpcClient>,
        chain_meta_service: Arc<ChainMetaService>,
        oo_receiver: Receiver<OpenOrders>,
        ob_receiver: Receiver<Arc<OrderBook>>,
        shutdown_receiver: Receiver<bool>,
        market_state: MarketStateV2,
        signer: Arc<Keypair>,
        cypher_user_pubkey: Pubkey,
        open_orders_pubkey: Pubkey,
    ) -> Self {
        Self {
            symbol,
            rpc_client,
            chain_meta_service,
            oo_receiver: Mutex::new(oo_receiver),
            ob_receiver: Mutex::new(ob_receiver),
            shutdown_receiver: Mutex::new(shutdown_receiver),
            market_state: Some(market_state),
            signer,
            cypher_user_pubkey,
            open_orders_pubkey,
            ..OrderManager::default()
        }
    }

    pub async fn start(self: &Arc<Self>) {
        let mut oo_receiver = self.oo_receiver.lock().await;
        let mut ob_receiver = self.ob_receiver.lock().await;
        let mut shutdown = self.shutdown_receiver.lock().await;
        let mut shutdown_signal: bool = false;

        loop {
            tokio::select! {
                oo = oo_receiver.recv() => {
                    if oo.is_err() {
                        warn!("[ORDERMGR-{}] There was an error while processing open orders updates, restarting loop.", self.symbol);
                        continue;
                    } else {
                        self._process_oo_update(oo.unwrap()).await;
                    }

                },
                ob = ob_receiver.recv() => {
                    if ob.is_err() {
                        warn!("[ORDERMGR-{}] There was an error while processing order book updates, restarting loop.", self.symbol);
                        continue;
                    } else {
                        self._process_ob_update(ob.unwrap()).await;
                    }
                }
                _ = shutdown.recv() => {
                    shutdown_signal = true;
                }
            }

            if shutdown_signal {
                info!(
                    "[ORDERMGR-{}] Received shutdown signal, stopping.",
                    self.symbol
                );
                break;
            }
        }
    }

    async fn _process_oo_update(self: &Arc<Self>, oo: OpenOrders) {
        info!("[ORDERMGR-{}] Received open orders update.", self.symbol);
        *self.open_orders.write().await = Some(oo);
    }

    async fn _process_ob_update(self: &Arc<Self>, ob: Arc<OrderBook>) {
        info!("[ORDERMGR-{}] Received order book update.", self.symbol);
        let bids = ob.bids.read().await;
        let asks = ob.asks.read().await;
        if asks.is_empty() && bids.is_empty() {
            info!("[ORDERMGR-{}] Latest ob for market is empty!", self.symbol)
        } else if asks.is_empty() && !bids.is_empty() {
            info!(
                "[ORDERMGR-{}] Latest ob for market: {} bids / best bid {}@{} - 0 asks ",
                self.symbol,
                bids.len(),
                bids[0].quantity,
                bids[0].price,
            );
        } else if bids.is_empty() && !asks.is_empty() {
            info!(
                "[ORDERMGR-{}] Latest ob for market: 0 bids - {} asks / best ask {}@{}",
                self.symbol,
                asks.len(),
                asks[0].quantity,
                asks[0].price,
            );
        } else {
            info!("[ORDERMGR-{}] Latest ob for market: {} bids / best bid {}@{} - {} asks / best ask {}@{}",
                self.symbol,
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

    async fn get_orders(self: &Arc<Self>) -> Vec<ManagedOrder> {
        let maybe_oo = *self.open_orders.read().await;
        let oo = match maybe_oo {
            Some(oo) => oo,
            None => {
                return Vec::new();
            }
        };
        let ob = self.orderbook.read().await;
        let orders = get_open_orders_with_qty(&oo, &ob).await;

        info!(
            "[ORDERMGR-{}] Found {} orders resting.",
            self.symbol,
            orders.len()
        );
        orders
    }

    async fn get_stale_orders(
        self: &Arc<Self>,
        quote_vols: &QuoteVolumes,
        best_bid: u64,
        best_ask: u64,
    ) -> Vec<ManagedOrder> {
        let mut stale_orders: Vec<ManagedOrder> = Vec::new();
        let orders = self.get_orders().await;

        for order in orders {
            if order.side == Side::Ask
                && (order.price != best_ask || order.quantity != quote_vols.ask_size as u64)
            {
                stale_orders.push(ManagedOrder {
                    order_id: order.order_id,
                    client_order_id: order.client_order_id,
                    price: order.price,
                    quantity: order.quantity,
                    side: Side::Ask,
                });
            } else if order.side == Side::Bid
                && (order.price != best_bid || order.quantity != quote_vols.bid_size as u64)
            {
                stale_orders.push(ManagedOrder {
                    order_id: order.order_id,
                    client_order_id: order.client_order_id,
                    price: order.price,
                    quantity: order.quantity,
                    side: Side::Bid,
                });
            }
        }

        info!(
            "[ORDERMGR-{}] Found {} stale orders resting.",
            self.symbol,
            stale_orders.len()
        );
        stale_orders
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_orders(
        self: &Arc<Self>,
        cypher_group: &CypherGroup,
        cypher_market: &CypherMarket,
        cypher_token: &CypherToken,
        signer: &Keypair,
        quote_vols: &QuoteVolumes,
        best_bid: u64,
        best_ask: u64,
    ) -> Result<(), MarketMakerError> {
        let mut ixs: Vec<Instruction> = Vec::new();
        let orders = self.get_orders().await;
        let stale_orders = self.get_stale_orders(quote_vols, best_bid, best_ask).await;

        if !stale_orders.is_empty() {
            let cancel_ixs = self
                .get_cancel_orders_ixs(
                    &stale_orders,
                    cypher_group,
                    cypher_market,
                    cypher_token,
                    signer,
                )
                .await;
            info!(
                "[ORDERMGR-{}] Cancelling {} stale orders.",
                self.symbol,
                cancel_ixs.len()
            );
            ixs.extend(cancel_ixs);
        }

        if orders.is_empty() || !stale_orders.is_empty() {
            let new_order_ixs = self
                .get_new_orders_ixs(
                    cypher_group,
                    cypher_market,
                    cypher_token,
                    quote_vols,
                    signer,
                    best_bid,
                    best_ask,
                )
                .await;
            info!(
                "[ORDERMGR-{}] Submitting {} new orders.",
                self.symbol,
                new_order_ixs.len()
            );
            ixs.extend(new_order_ixs);
        }

        if !ixs.is_empty() {
            match self.submit_orders(ixs, signer).await {
                Ok(_) => (),
                Err(e) => {
                    return Err(e);
                }
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn cancel_orders_remain_neutral(
        self: &Arc<Self>,
        cypher_group: &CypherGroup,
        cypher_market: &CypherMarket,
        cypher_token: &CypherToken,
        signer: &Keypair,
        quote_vols: &QuoteVolumes,
        best_bid: u64,
        best_ask: u64,
    ) -> Result<(), MarketMakerError> {
        let mut ixs: Vec<Instruction> = Vec::new();
        let orders = self.get_orders().await;

        let cancel_ixs = self
            .get_cancel_orders_ixs(&orders, cypher_group, cypher_market, cypher_token, signer)
            .await;
        info!(
            "[ORDERMGR-{}] Cancelling {} stale orders.",
            self.symbol,
            cancel_ixs.len()
        );
        ixs.extend(cancel_ixs);

        if !ixs.is_empty() {
            match self.submit_orders(ixs, signer).await {
                Ok(_) => (),
                Err(e) => {
                    return Err(e);
                }
            }
        }

        Ok(())
    }

    async fn get_cancel_orders_ixs(
        self: &Arc<Self>,
        stale_orders: &Vec<ManagedOrder>,
        cypher_group: &CypherGroup,
        cypher_market: &CypherMarket,
        cypher_token: &CypherToken,
        signer: &Keypair,
    ) -> Vec<Instruction> {
        let inflight_orders = self.inflight_orders.read().await;
        let mut cancelling_orders = inflight_orders.cancelling_orders.write().await;
        let mut ixs: Vec<Instruction> = Vec::new();

        for order in stale_orders {
            info!(
                "[ORDERMGR-{}] Cancelling order with id {}",
                self.symbol, order.order_id
            );
            ixs.push(get_cancel_order_ix(
                cypher_group,
                cypher_market,
                cypher_token,
                &self.market_state.unwrap(),
                &self.open_orders_pubkey,
                &self.cypher_user_pubkey,
                signer,
                CancelOrderInstructionV2 {
                    order_id: order.order_id,
                    side: order.side,
                },
            ));
            cancelling_orders.push(order.client_order_id);
        }

        drop(cancelling_orders);
        drop(inflight_orders);
        ixs
    }

    #[allow(clippy::too_many_arguments)]
    async fn get_new_orders_ixs(
        self: &Arc<Self>,
        cypher_group: &CypherGroup,
        cypher_market: &CypherMarket,
        cypher_token: &CypherToken,
        quote_vols: &QuoteVolumes,
        signer: &Keypair,
        best_bid: u64,
        best_ask: u64,
    ) -> Vec<Instruction> {
        let mut ixs: Vec<Instruction> = Vec::new();
        let inflight_orders = self.inflight_orders.read().await;
        let mut new_orders = inflight_orders.new_orders.write().await;

        if quote_vols.ask_size > 0 {
            let max_native_pc_qty_ask = quote_vols.ask_size as u64 * best_ask;
            let client_order_id = *self.client_order_id.read().await;
            info!(
                "[ORDERMGR-{}] Submitting new ask at {} for {} units at max qty pc {} with coid: {}",
                self.symbol, best_ask, quote_vols.ask_size, max_native_pc_qty_ask, client_order_id
            );

            ixs.push(get_new_order_ix(
                cypher_group,
                cypher_market,
                cypher_token,
                &self.market_state.unwrap(),
                &self.open_orders_pubkey,
                &self.cypher_user_pubkey,
                signer,
                NewOrderInstructionV3 {
                    client_order_id,
                    limit: u16::MAX,
                    limit_price: NonZeroU64::new(best_ask).unwrap(),
                    side: Side::Ask,
                    max_coin_qty: NonZeroU64::new(quote_vols.ask_size as u64).unwrap(),
                    max_native_pc_qty_including_fees: NonZeroU64::new(max_native_pc_qty_ask)
                        .unwrap(),
                    order_type: OrderType::PostOnly,
                    self_trade_behavior: SelfTradeBehavior::CancelProvide,
                    max_ts: i64::MAX,
                },
            ));
            *self.client_order_id.write().await += 1;
            new_orders.push(client_order_id);
        }

        if quote_vols.bid_size > 0 {
            let max_native_pc_qty_bid = quote_vols.bid_size as u64 * best_bid;
            let client_order_id = *self.client_order_id.read().await;
            info!(
                "[ORDERMGR-{}] Submitting new bid at {} for {} units at max pc qty {} with coid: {}",
                self.symbol, best_bid, quote_vols.bid_size, max_native_pc_qty_bid, client_order_id
            );

            ixs.push(get_new_order_ix(
                cypher_group,
                cypher_market,
                cypher_token,
                &self.market_state.unwrap(),
                &self.open_orders_pubkey,
                &self.cypher_user_pubkey,
                signer,
                NewOrderInstructionV3 {
                    client_order_id,
                    limit: u16::MAX,
                    limit_price: NonZeroU64::new(best_bid).unwrap(),
                    side: Side::Bid,
                    max_coin_qty: NonZeroU64::new(quote_vols.bid_size as u64).unwrap(),
                    max_native_pc_qty_including_fees: NonZeroU64::new(max_native_pc_qty_bid)
                        .unwrap(),
                    order_type: OrderType::PostOnly,
                    self_trade_behavior: SelfTradeBehavior::CancelProvide,
                    max_ts: i64::MAX,
                },
            ));
            *self.client_order_id.write().await += 1;
            new_orders.push(client_order_id);
        }

        drop(new_orders);
        drop(inflight_orders);
        ixs
    }

    async fn submit_orders(
        self: &Arc<Self>,
        ixs: Vec<Instruction>,
        signer: &Keypair,
    ) -> Result<(), MarketMakerError> {
        let blockhash = self.chain_meta_service.get_latest_blockhash().await;
        if blockhash == Hash::default() {
            return Err(MarketMakerError::ErrorSubmittingOrders);
        };

        info!("[ORDERMGR-{}] Using blockhash {}", self.symbol, blockhash);

        let res = self.submit_transactions(ixs, signer, blockhash).await;
        match res {
            Ok(s) => {
                for sig in s {
                    info!(
                        "[ORDERMGR-{}] Successfully submitted transaction: {}",
                        self.symbol, sig
                    );
                }
            }
            Err(e) => {
                warn!(
                    "[ORDERMGR-{}] An error occurred while submitting orders: {}",
                    self.symbol,
                    e.to_string()
                );
                return Err(MarketMakerError::ErrorSubmittingOrders);
            }
        };

        Ok(())
    }

    async fn submit_transactions(
        self: &Arc<Self>,
        ixs: Vec<Instruction>,
        signer: &Keypair,
        blockhash: Hash,
    ) -> Result<Vec<Signature>, ClientError> {
        let mut txn_builder = FastTxnBuilder::new();
        let mut submitted: bool = false;
        let mut signatures: Vec<Signature> = Vec::new();
        let mut prev_tx: Transaction = Transaction::default();

        for ix in ixs {
            if txn_builder.len() != 0 {
                let tx = txn_builder.build(blockhash, signer, None);
                // we do this to attempt to pack as many ixs in a tx as possible
                // there's more efficient ways to do it but we'll do it in the future
                if tx.message_data().len() > 1000 {
                    let res = self.send_and_confirm_transaction(&prev_tx).await;
                    match res {
                        Ok(s) => {
                            submitted = true;
                            txn_builder.clear();
                            signatures.push(s);
                        }
                        Err(e) => {
                            warn!("[ORDERMGR-{}] There was an error submitting transaction and waiting for confirmation: {}",
                            self.symbol, e.to_string());
                        }
                    }
                } else {
                    txn_builder.add(ix);
                    prev_tx = tx;
                }
            } else {
                txn_builder.add(ix);
            }
        }

        if !submitted {
            let tx = txn_builder.build(blockhash, signer, None);
            let res = self.send_and_confirm_transaction(&tx).await;
            match res {
                Ok(_) => (),
                Err(e) => {
                    warn!("[ORDERMGR-{}] There was an error submitting transaction and waiting for confirmation: {}", self.symbol, e.to_string());
                    return Err(e);
                }
            }
        }

        Ok(signatures)
    }

    async fn send_and_confirm_transaction(
        self: &Arc<Self>,
        tx: &Transaction,
    ) -> Result<Signature, ClientError> {
        let submit_res = self.rpc_client.send_and_confirm_transaction(tx).await;

        match submit_res {
            Ok(s) => {
                info!(
                    "[ORDERMGR-{}] Successfully submitted transaction. Transaction signature: {}",
                    self.symbol,
                    s.to_string()
                );
                Ok(s)
            }
            Err(e) => {
                warn!(
                    "[ORDERMGR-{}] There was an error submitting transaction: {}",
                    self.symbol,
                    e.to_string()
                );
                Err(e)
            }
        }
    }
}

async fn get_open_orders(open_orders: &OpenOrders) -> Vec<ManagedOrder> {
    let mut oo: Vec<ManagedOrder> = Vec::new();
    let orders = open_orders.orders;

    for i in 0..orders.len() {
        let order_id = open_orders.orders[i];
        let client_order_id = open_orders.client_order_ids[i];

        if order_id != u128::default() {
            let price = (order_id >> 64) as u64;
            let side = open_orders.slot_side(i as u8).unwrap();

            oo.push(ManagedOrder {
                order_id,
                client_order_id,
                side,
                price,
                quantity: u64::default(),
            });
        }
    }

    oo
}

async fn get_open_orders_with_qty(
    open_orders: &OpenOrders,
    orderbook: &OrderBook,
) -> Vec<ManagedOrder> {
    let mut oo: Vec<ManagedOrder> = Vec::new();
    let orders = open_orders.orders;

    for i in 0..orders.len() {
        let order_id = open_orders.orders[i];
        let client_order_id = open_orders.client_order_ids[i];

        if order_id != u128::default() {
            let price = (order_id >> 64) as u64;
            let side = open_orders.slot_side(i as u8).unwrap();
            let ob_order = get_order_book_line(orderbook, order_id, side).await;

            if ob_order.is_some() {
                oo.push(ManagedOrder {
                    order_id,
                    client_order_id,
                    side,
                    price,
                    quantity: ob_order.unwrap().quantity,
                });
            }
        }
    }

    oo
}

async fn get_order_book_line(
    orderbook: &OrderBook,
    order_id: u128,
    side: Side,
) -> Option<OrderBookOrder> {
    if side == Side::Ask {
        for order in orderbook.asks.read().await.iter() {
            if order.order_id == order_id {
                return Some(OrderBookOrder {
                    order_id: order.order_id,
                    price: order.price,
                    quantity: order.quantity,
                    client_order_id: order.client_order_id,
                });
            }
        }
    }

    if side == Side::Bid {
        for order in orderbook.bids.read().await.iter() {
            if order.order_id == order_id {
                return Some(OrderBookOrder {
                    order_id: order.order_id,
                    price: order.price,
                    quantity: order.quantity,
                    client_order_id: order.client_order_id,
                });
            }
        }
    }

    None
}
