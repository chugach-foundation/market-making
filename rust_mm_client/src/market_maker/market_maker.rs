#![allow(dead_code)]
use {
    super::order_manager::OrderManager,
    super::{InventoryManager, Worker, WorkerConfig},
    crate::providers::{CypherAccountProvider, CypherGroupProvider, OpenOrdersProvider},
    crate::{
        accounts_cache::AccountsCache,
        config::{cypher_config::CypherConfig, MarketMakerConfig},
        providers::orderbook_provider::{OrderBook, OrderBookProvider},
        services::{AccountInfoService, ChainMetaService},
        MarketMakerError,
    },
    anchor_lang::AnchorDeserialize,
    cypher::utils::{derive_open_orders_address, parse_dex_account},
    cypher::{CypherGroup, CypherUser},
    log::{info, warn},
    safe_transmute::transmute_to_bytes,
    serum_dex::state::{MarketStateV2, OpenOrders},
    solana_client::{client_error::ClientError, nonblocking::rpc_client::RpcClient},
    solana_sdk::signature::Keypair,
    solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey},
    std::convert::identity,
    std::{str::FromStr, sync::Arc},
    tokio::sync::broadcast::{channel, Receiver, Sender},
    tokio::task::JoinHandle,
};

pub struct MarketMaker {
    // services
    rpc_client: Arc<RpcClient>,
    /// polling keys is the keys used by the account info service
    polling_keys: Vec<Pubkey>,
    ai_service: Arc<AccountInfoService>,
    cm_service: Arc<ChainMetaService>,
    inventory_manager: Arc<InventoryManager>,
    order_manager: Arc<OrderManager>,

    // providers
    accounts_cache: AccountsCacheWrapper,
    cypher_account_provider: CypherAccountProviderWrapper,
    cypher_group_provider: CypherGroupProviderWrapper,
    orderbook_provider: OrderBookProviderWrapper,
    open_orders_provider: OpenOrdersProviderWrapper,

    // the worker
    worker: Worker,

    // the configs
    config: Arc<MarketMakerConfig>,
    cypher_config: Arc<CypherConfig>,

    owner_keypair: Keypair,
    cypher_user_pubkey: Pubkey,
    cypher_user: Box<CypherUser>,
    cypher_group_pubkey: Pubkey,
    cypher_group: Box<CypherGroup>,

    // async tasks
    shutdown_sender: Arc<Sender<bool>>,
    tasks: Vec<JoinHandle<()>>,
}

#[allow(clippy::too_many_arguments)]
impl MarketMaker {
    pub fn new(
        rpc_client: Arc<RpcClient>,
        config: Arc<MarketMakerConfig>,
        cypher_config: Arc<CypherConfig>,
        cypher_group: Box<CypherGroup>,
        cypher_group_pubkey: Pubkey,
        owner_keypair: Keypair,
        cypher_user: Box<CypherUser>,
        cypher_user_pubkey: Pubkey,
        shutdown_sender: Sender<bool>,
    ) -> Self {
        Self {
            rpc_client,
            config,
            cypher_config,
            owner_keypair,
            cypher_user,
            cypher_user_pubkey,
            cypher_group,
            cypher_group_pubkey,
            shutdown_sender: Arc::new(shutdown_sender),
            tasks: Vec::new(),
            worker: Worker::default(),
            polling_keys: Vec::new(),
            accounts_cache: AccountsCacheWrapper::default(),
            cypher_account_provider: CypherAccountProviderWrapper::default(),
            cypher_group_provider: CypherGroupProviderWrapper::default(),
            orderbook_provider: OrderBookProviderWrapper::default(),
            open_orders_provider: OpenOrdersProviderWrapper::default(),
            ai_service: Arc::new(AccountInfoService::default()),
            cm_service: Arc::new(ChainMetaService::default()),
            inventory_manager: Arc::new(InventoryManager::default()),
            order_manager: Arc::new(OrderManager::default()),
        }
    }

    /// start all of the necessary services and providers and initialize the market maker's workers
    pub async fn start(mut self) -> Result<(), MarketMakerError> {
        // init the services before actually starting them
        info!("Initializing services.");

        match self.init_services().await {
            Ok(_) => (),
            Err(_) => {
                warn!("There was an error while initializing services.");
                return Err(MarketMakerError::InitServicesError);
            }
        };

        // start the services
        let ai_t = tokio::spawn(async move {
            self.ai_service.start_service().await;
        });
        self.tasks.push(ai_t);

        let cm_t = tokio::spawn(async move {
            self.cm_service.start_service().await;
        });
        self.tasks.push(cm_t);

        // start the providers
        let group_t = tokio::spawn(async move {
            self.cypher_group_provider.provider.start().await;
        });
        self.tasks.push(group_t);

        let obp_t = tokio::spawn(async move {
            self.orderbook_provider.provider.start().await;
        });
        self.tasks.push(obp_t);

        let ca_t = tokio::spawn(async move {
            self.cypher_account_provider.provider.start().await;
        });
        self.tasks.push(ca_t);

        let oo_t = tokio::spawn(async move {
            self.open_orders_provider.provider.start().await;
        });
        self.tasks.push(oo_t);

        let om_t = tokio::spawn(async move {
            self.order_manager.start().await;
        });
        self.tasks.push(om_t);

        let worker_t = tokio::spawn(async move {
            self.worker.set_keypair(self.owner_keypair);
            self.worker.start().await;
        });
        self.tasks.push(worker_t);

        for task in self.tasks {
            let res = tokio::join!(task);

            match res {
                (Ok(_),) => (),
                (Err(e),) => {
                    warn!("There was an error joining with task: {}", e.to_string());
                }
            };
        }

        Ok(())
    }

    /// initialize the services necessary for the market maker to operate
    async fn init_services(&mut self) -> Result<(), MarketMakerError> {
        // unbounded channel for the accounts cache to send messages whenever a given account gets updated
        let (accounts_cache_s, accounts_cache_r) = channel::<Pubkey>(u16::MAX as usize);
        self.accounts_cache.sender = accounts_cache_s;
        self.accounts_cache.receiver = accounts_cache_r;

        self.accounts_cache.cache =
            Arc::new(AccountsCache::new(self.accounts_cache.sender.clone()));

        self.cm_service = Arc::new(ChainMetaService::new(
            Arc::clone(&self.rpc_client),
            self.shutdown_sender.subscribe(),
        ));

        // process the market configs before proceeding
        let pcfg_res = self.process_market_configs().await;
        match pcfg_res {
            Ok(_) => (),
            Err(_) => {
                warn!("There was an error processing market configs.");
                return Err(MarketMakerError::InitServicesError);
            }
        };

        self.polling_keys.push(self.cypher_user_pubkey);
        self.polling_keys.push(self.cypher_group_pubkey);

        self.ai_service = Arc::new(AccountInfoService::new(
            Arc::clone(&self.accounts_cache.cache),
            Arc::clone(&self.rpc_client),
            &self.polling_keys,
            self.shutdown_sender.subscribe(),
        ));

        Ok(())
    }

    /// process the markets provided in the config and prepare the configs for workers and services
    async fn process_market_configs(&mut self) -> Result<(), MarketMakerError> {
        let mm_market_config = &self.config.market;
        let group_config = self
            .cypher_config
            .get_group(self.config.group.as_str())
            .unwrap();

        // get the keys necessary to initialize the account info service
        let market_config = group_config
            .get_market(mm_market_config.name.as_str())
            .unwrap();

        let market_pubkey = Pubkey::from_str(market_config.address.as_str()).unwrap();
        let market_bids = Pubkey::from_str(market_config.bids.as_str()).unwrap();
        let market_asks = Pubkey::from_str(market_config.asks.as_str()).unwrap();

        // add the market related pubkeys to the vec of keys to poll
        let poll_market_keys = &mut vec![market_pubkey, market_bids, market_asks];

        let market_res = get_serum_market(Arc::clone(&self.rpc_client), market_pubkey).await;

        let market_state = match market_res {
            Ok(m) => m,
            Err(_) => return Err(MarketMakerError::ErrorFetchingDexMarket),
        };

        let open_orders_pubkey =
            derive_open_orders_address(&market_pubkey, &self.cypher_user_pubkey).0;
        self.polling_keys.push(open_orders_pubkey);

        let c_asset_mint =
            Pubkey::try_from_slice(transmute_to_bytes(&identity(market_state.coin_mint))).unwrap();

        let worker_config = WorkerConfig {
            market: market_pubkey,
            c_asset_mint,
            market_index: market_config.market_index,
            symbol: market_config.name.to_string(),
        };

        let (ob_s, ob_r) = channel::<Arc<OrderBook>>(u16::MAX as usize);
        let arc_ob_s = Arc::new(ob_s);
        let ob_provider = Arc::new(OrderBookProvider::new(
            Arc::clone(&self.accounts_cache.cache),
            Arc::clone(&arc_ob_s),
            self.accounts_cache.sender.subscribe(),
            self.shutdown_sender.subscribe(),
            market_pubkey,
            market_bids,
            market_asks,
            market_state.coin_lot_size,
            market_state.pc_lot_size,
            0_u64,
        ));

        let (ca_s, ca_r) = channel::<Box<CypherUser>>(u16::MAX as usize);
        let arc_ca_s = Arc::new(ca_s);
        let ca_provider = Arc::new(CypherAccountProvider::new(
            Arc::clone(&self.accounts_cache.cache),
            Arc::clone(&arc_ca_s),
            self.accounts_cache.sender.subscribe(),
            self.shutdown_sender.subscribe(),
            self.cypher_user_pubkey,
        ));

        let (cg_s, cg_r) = channel::<Box<CypherGroup>>(u16::MAX as usize);
        let arc_cg_s = Arc::new(cg_s);
        let cg_provider = Arc::new(CypherGroupProvider::new(
            Arc::clone(&self.accounts_cache.cache),
            Arc::clone(&arc_cg_s),
            self.accounts_cache.sender.subscribe(),
            self.shutdown_sender.subscribe(),
            self.cypher_group_pubkey,
        ));

        let (oo_s, oo_r) = channel::<OpenOrders>(u16::MAX as usize);
        let arc_oo_s = Arc::new(oo_s);
        let oo_provider = Arc::new(OpenOrdersProvider::new(
            Arc::clone(&self.accounts_cache.cache),
            Arc::clone(&arc_oo_s),
            self.accounts_cache.sender.subscribe(),
            self.shutdown_sender.subscribe(),
            open_orders_pubkey,
        ));

        self.inventory_manager = Arc::new(InventoryManager::new(
            Arc::clone(&self.config),
            market_config.market_index as usize,
            self.config.inventory_manager_config.max_quote,
            self.config.inventory_manager_config.shape_num,
            self.config.inventory_manager_config.shape_denom,
            self.config.inventory_manager_config.spread,
        ));

        self.order_manager = Arc::new(OrderManager::new(
            worker_config.symbol.to_string(),
            Arc::clone(&self.rpc_client),
            Arc::clone(&self.cm_service),
            arc_oo_s.subscribe(),
            arc_ob_s.subscribe(),
            self.shutdown_sender.subscribe(),
            market_state,
        ));

        self.worker = Worker::new(
            worker_config,
            Arc::clone(&self.inventory_manager),
            Arc::clone(&self.order_manager),
            arc_ca_s.subscribe(),
            arc_cg_s.subscribe(),
            Arc::clone(&self.shutdown_sender),
            self.cypher_user_pubkey,
            open_orders_pubkey,
        );

        self.orderbook_provider = OrderBookProviderWrapper {
            provider: ob_provider,
            sender: arc_ob_s,
            receiver: ob_r,
        };

        self.cypher_account_provider = CypherAccountProviderWrapper {
            provider: ca_provider,
            sender: arc_ca_s,
            receiver: ca_r,
        };

        self.open_orders_provider = OpenOrdersProviderWrapper {
            provider: oo_provider,
            sender: arc_oo_s,
            receiver: oo_r,
        };

        self.cypher_group_provider = CypherGroupProviderWrapper {
            provider: cg_provider,
            sender: arc_cg_s,
            receiver: cg_r,
        };

        self.polling_keys.append(poll_market_keys);

        Ok(())
    }
}

async fn get_serum_market(
    client: Arc<RpcClient>,
    market: Pubkey,
) -> Result<MarketStateV2, ClientError> {
    let ai_res = client
        .get_account_with_commitment(&market, CommitmentConfig::confirmed())
        .await;

    let ai = match ai_res {
        Ok(ai) => ai.value.unwrap(),
        Err(e) => {
            warn!(
                "There was an error while fetching the serum market: {}",
                e.to_string()
            );
            return Err(e);
        }
    };

    let market = parse_dex_account(ai.data);

    Ok(market)
}

struct AccountsCacheWrapper {
    cache: Arc<AccountsCache>,
    sender: Sender<Pubkey>,
    receiver: Receiver<Pubkey>,
}

impl AccountsCacheWrapper {
    pub fn default() -> Self {
        Self {
            cache: Arc::new(AccountsCache::default()),
            sender: channel::<Pubkey>(u16::MAX as usize).0,
            receiver: channel::<Pubkey>(u16::MAX as usize).1,
        }
    }
}

struct OrderBookProviderWrapper {
    provider: Arc<OrderBookProvider>,
    sender: Arc<Sender<Arc<OrderBook>>>,
    receiver: Receiver<Arc<OrderBook>>,
}

impl OrderBookProviderWrapper {
    pub fn default() -> Self {
        Self {
            provider: Arc::new(OrderBookProvider::default()),
            sender: Arc::new(channel::<Arc<OrderBook>>(u16::MAX as usize).0),
            receiver: channel::<Arc<OrderBook>>(u16::MAX as usize).1,
        }
    }
}

struct OpenOrdersProviderWrapper {
    provider: Arc<OpenOrdersProvider>,
    sender: Arc<Sender<OpenOrders>>,
    receiver: Receiver<OpenOrders>,
}

impl OpenOrdersProviderWrapper {
    pub fn default() -> Self {
        Self {
            provider: Arc::new(OpenOrdersProvider::default()),
            sender: Arc::new(channel::<OpenOrders>(u16::MAX as usize).0),
            receiver: channel::<OpenOrders>(u16::MAX as usize).1,
        }
    }
}

struct CypherAccountProviderWrapper {
    provider: Arc<CypherAccountProvider>,
    sender: Arc<Sender<Box<CypherUser>>>,
    receiver: Receiver<Box<CypherUser>>,
}

impl CypherAccountProviderWrapper {
    pub fn default() -> Self {
        Self {
            provider: Arc::new(CypherAccountProvider::default()),
            sender: Arc::new(channel::<Box<CypherUser>>(u16::MAX as usize).0),
            receiver: channel::<Box<CypherUser>>(u16::MAX as usize).1,
        }
    }
}

struct CypherGroupProviderWrapper {
    provider: Arc<CypherGroupProvider>,
    sender: Arc<Sender<Box<CypherGroup>>>,
    receiver: Receiver<Box<CypherGroup>>,
}

impl CypherGroupProviderWrapper {
    pub fn default() -> Self {
        Self {
            provider: Arc::new(CypherGroupProvider::default()),
            sender: Arc::new(channel::<Box<CypherGroup>>(u16::MAX as usize).0),
            receiver: channel::<Box<CypherGroup>>(u16::MAX as usize).1,
        }
    }
}
