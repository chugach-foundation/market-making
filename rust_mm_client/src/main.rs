mod accounts_cache;
mod config;
mod fast_tx_builder;
mod logging;
mod market_maker;
mod math;
mod providers;
mod serum_slab;
mod services;
mod utils;

use clap::Parser;
use config::*;
use cypher::{
    constants::QUOTE_TOKEN_IDX,
    quote_mint,
    utils::{
        derive_cypher_user_address, derive_open_orders_address, get_zero_copy_account,
        parse_dex_account,
    },
    CypherGroup, CypherUser,
};
use fast_tx_builder::FastTxnBuilder;
use faucet::get_request_airdrop_ix;
use jet_proto_math::Number;
use log::{info, warn};
use logging::init_logger;
use serum_dex::state::OpenOrders;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig, pubkey::Pubkey, signature::Keypair, signer::Signer,
};
use spl_associated_token_account::instruction::create_associated_token_account;
use std::{fs::File, io::Read, str::FromStr, sync::Arc};
use tokio::sync::broadcast::channel;
use utils::{
    derive_quote_token_address, get_init_open_orders_ix, get_token_account, init_cypher_user,
};

use crate::{market_maker::MarketMaker, utils::get_deposit_collateral_ix};

// rework this, maybe ask user for input as well
pub const CYPHER_CONFIG_PATH: &str = "./cfg/group.json";

#[derive(Parser)]
struct Cli {
    #[clap(short = 'c', long = "config", parse(from_os_str))]
    config: std::path::PathBuf,
}

#[derive(Debug)]
pub enum MarketMakerError {
    ConfigLoadError,
    ErrorFetchingDexMarket,
    ErrorFetchingCypherGroup,
    ErrorFetchingCypherAccount,
    ErrorFetchingOpenOrders,
    ErrorCreatingCypherAccount,
    ErrorCreatingOpenOrders,
    ErrorDepositing,
    ErrorSubmittingOrders,
    ChannelSendError,
    JoiningTaskError,
    InitServicesError,
    RpcClientInitError,
    PubsubClientInitError,
    KeypairFileOpenError,
    KeypairFileReadError,
    KeypairLoadError,
    ShutdownError,
}

fn load_keypair(path: &str) -> Result<Keypair, MarketMakerError> {
    let fd = File::open(path);

    let mut file = match fd {
        Ok(f) => f,
        Err(e) => {
            warn!("Failed to load keypair file: {}", e.to_string());
            return Err(MarketMakerError::KeypairFileOpenError);
        }
    };

    let file_string = &mut String::new();
    let file_read_res = file.read_to_string(file_string);

    let _ = if let Err(e) = file_read_res {
        warn!(
            "Failed to read keypair bytes from keypair file: {}",
            e.to_string()
        );
        return Err(MarketMakerError::KeypairFileReadError);
    };

    let keypair_bytes: Vec<u8> = file_string
        .replace('[', "")
        .replace(']', "")
        .replace(',', " ")
        .split(' ')
        .map(|x| u8::from_str(x).unwrap())
        .collect();

    let keypair = Keypair::from_bytes(keypair_bytes.as_ref());

    match keypair {
        Ok(kp) => Ok(kp),
        Err(e) => {
            warn!("Failed to load keypair from bytes: {}", e.to_string());
            Err(MarketMakerError::KeypairLoadError)
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), MarketMakerError> {
    let args = Cli::parse();

    _ = init_logger();

    // load config
    let config_path = args.config.as_path().to_str().unwrap();

    info!("Loading config from {}", config_path);

    let mm_config = Arc::new(load_mm_config(config_path).unwrap());
    let cypher_config = Arc::new(load_cypher_config(CYPHER_CONFIG_PATH).unwrap());

    let cluster_config = cypher_config.get_config_for_cluster(mm_config.group.as_str());

    let keypair = load_keypair(mm_config.wallet.as_str()).unwrap();
    let pubkey = keypair.pubkey();
    info!("Loaded keypair with pubkey: {}", pubkey.to_string());

    let cypher_group_config = Arc::new(cypher_config.get_group(mm_config.group.as_str()).unwrap());

    let cypher_group_key = Pubkey::from_str(cypher_group_config.address.as_str()).unwrap();

    // initialize rpc client with cluster and cluster url provided in config
    info!(
        "Initializing rpc client for cluster-{} with url: {}",
        mm_config.group, cluster_config.rpc_url
    );
    let rpc_client = Arc::new(RpcClient::new_with_commitment(
        cluster_config.rpc_url.to_string(),
        CommitmentConfig::confirmed(),
    ));

    info!(
        "Attempting to get the cypher group account with key: {}",
        cypher_group_key
    );
    let cypher_group_res = _get_cypher_group(Arc::clone(&rpc_client), cypher_group_key).await;
    let cypher_group = match cypher_group_res {
        Ok(cg) => cg,
        Err(_) => {
            warn!("An error occurred while fetching the cypher group.");
            return Err(MarketMakerError::ErrorFetchingCypherGroup);
        }
    };

    let (cypher_user_key, _bump) = derive_cypher_user_address(&cypher_group_key, &keypair.pubkey());

    info!(
        "Attempting to get the cypher user account with key: {}",
        cypher_user_key
    );
    let cypher_account_res = _get_or_init_cypher_user(
        &keypair,
        &cypher_group_key,
        &cypher_group,
        &cypher_user_key,
        Arc::clone(&rpc_client),
        &mm_config,
    )
    .await;
    let cypher_account = match cypher_account_res {
        Ok(cg) => cg,
        Err(_) => {
            warn!("An error occurred while getting or creating the cypher user account.");
            return Err(MarketMakerError::ErrorCreatingCypherAccount);
        }
    };

    let market_config = cypher_group_config
        .get_market(mm_config.market.name.as_str())
        .unwrap();

    let market_pubkey = Pubkey::from_str(market_config.address.as_str()).unwrap();
    let open_orders = derive_open_orders_address(&market_pubkey, &cypher_user_key).0;

    info!(
        "Attempting to get the open orders account for market: {}",
        market_config.address
    );
    let open_orders_res = _get_or_init_open_orders(
        &keypair,
        &cypher_group_key,
        &cypher_user_key,
        &market_pubkey,
        &open_orders,
        Arc::clone(&rpc_client),
    )
    .await;
    let _open_orders = match open_orders_res {
        Ok(cg) => cg,
        Err(_) => {
            warn!("An error occurred while getting or creating the open orders account.");
            return Err(MarketMakerError::ErrorCreatingOpenOrders);
        }
    };

    info!("Initializing market maker.");

    let (shutdown_send, mut _shutdown_recv) = channel::<bool>(1);

    let mm = MarketMaker::new(
        Arc::clone(&rpc_client),
        Arc::clone(&mm_config),
        Arc::clone(&cypher_config),
        cypher_group,
        cypher_group_key,
        keypair,
        cypher_account,
        cypher_user_key,
        shutdown_send.clone(),
    );

    let mm_t = tokio::spawn(async move {
        let start_res = mm.start().await;
        match start_res {
            Ok(_) => (),
            Err(e) => {
                return Err(e);
            }
        };

        Ok(())
    });

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            match shutdown_send.send(true) {
                Ok(_) => {
                    info!("Sucessfully sent shutdown signal. Waiting for tasks to complete...")
                },
                Err(e) => {
                    warn!("Failed to send shutdown error: {}", e.to_string());
                    return Err(MarketMakerError::ShutdownError);
                }
            };
        },
    };

    let (mm_res,) = tokio::join!(mm_t);

    match mm_res {
        Ok(_) => (),
        Err(e) => {
            warn!(
                "There was an error while shutting down the account info service: {}",
                e.to_string()
            );
            return Err(MarketMakerError::ShutdownError);
        }
    };

    Ok(())
}

async fn _get_cypher_group(
    rpc_client: Arc<RpcClient>,
    cypher_group_pubkey: Pubkey,
) -> Result<Box<CypherGroup>, MarketMakerError> {
    let res = rpc_client.get_account(&cypher_group_pubkey).await;
    let acc = match res {
        Ok(a) => Some(a),
        Err(e) => {
            warn!("Failed to fetch cypher group: {}", e.to_string());
            None
        }
    };

    let cypher_group = get_zero_copy_account::<CypherGroup>(&acc.unwrap());

    Ok(cypher_group)
}

async fn _get_or_init_open_orders(
    owner: &Keypair,
    cypher_group_pubkey: &Pubkey,
    cypher_user_pubkey: &Pubkey,
    cypher_market: &Pubkey,
    open_orders: &Pubkey,
    rpc_client: Arc<RpcClient>,
) -> Result<OpenOrders, MarketMakerError> {
    let account_state = _fetch_open_orders(open_orders, Arc::clone(&rpc_client)).await;

    if account_state.is_ok() {
        let acc = account_state.unwrap();
        info!(
            "Open orders account for market {} with key {} already exists.",
            cypher_market, open_orders
        );
        Ok(acc)
    } else {
        info!("Open orders account does not exist, creating..");

        let res = _init_open_orders(
            cypher_group_pubkey,
            cypher_user_pubkey,
            cypher_market,
            open_orders,
            owner,
            Arc::clone(&rpc_client),
        )
        .await;

        match res {
            Ok(()) => (),
            Err(e) => {
                warn!("An error occurred while creating the open orders account.");
                return Err(e);
            }
        };
        let open_orders = _fetch_open_orders(open_orders, Arc::clone(&rpc_client))
            .await
            .unwrap();
        Ok(open_orders)
    }
}

async fn _get_or_init_cypher_user(
    owner: &Keypair,
    cypher_group_pubkey: &Pubkey,
    cypher_group: &CypherGroup,
    cypher_user_pubkey: &Pubkey,
    rpc_client: Arc<RpcClient>,
    config: &MarketMakerConfig,
) -> Result<Box<CypherUser>, MarketMakerError> {
    let account_state = _fetch_cypher_user(cypher_user_pubkey, Arc::clone(&rpc_client)).await;

    if account_state.is_ok() {
        info!("Cypher user account already exists, checking balance meets the config criteria.");
        let account = account_state.unwrap();
        _check_cypher_balance(
            owner,
            cypher_user_pubkey,
            &account,
            cypher_group,
            config,
            rpc_client,
        )
        .await
        .unwrap();
        Ok(account)
    } else {
        info!("Cypher user account does not existing, creating account.");
        let res = _init_cypher_user(cypher_group_pubkey, owner, Arc::clone(&rpc_client)).await;

        match res {
            Ok(()) => (),
            Err(e) => {
                warn!("An error occurred while creating the cypher user account.");
                return Err(e);
            }
        };

        let cypher_user = _fetch_cypher_user(cypher_user_pubkey, Arc::clone(&rpc_client))
            .await
            .unwrap();

        _check_cypher_balance(
            owner,
            cypher_user_pubkey,
            &cypher_user,
            cypher_group,
            config,
            rpc_client,
        )
        .await
        .unwrap();

        Ok(cypher_user)
    }
}

async fn _check_cypher_balance(
    owner: &Keypair,
    cypher_user_pubkey: &Pubkey,
    cypher_user: &CypherUser,
    cypher_group: &CypherGroup,
    config: &MarketMakerConfig,
    rpc_client: Arc<RpcClient>,
) -> Result<(), MarketMakerError> {
    let position = cypher_user.get_position(QUOTE_TOKEN_IDX).unwrap();
    let quote_token = cypher_group.get_cypher_token(QUOTE_TOKEN_IDX).unwrap();

    let initial_capital_native: Number = (config.inventory_manager_config.initial_capital
        * 10_u64.checked_pow(quote_token.decimals().into()).unwrap())
    .into();
    info!(
        "Desired initial capitial (native): {}.",
        initial_capital_native
    );

    let total_quote_deposits = position.total_deposits(quote_token);
    info!(
        "User total quote deposits (native): {}.",
        total_quote_deposits
    );

    if total_quote_deposits >= initial_capital_native {
        return Ok(());
    };

    let amount_rem = initial_capital_native - total_quote_deposits;

    info!("Depositing quote token (native): {}.", amount_rem);

    if config.group.contains("devnet") {
        match request_airdrop(owner, Arc::clone(&rpc_client)).await {
            Ok(_) => (),
            Err(e) => {
                warn!("There was an error requesting airdrop: {:?}", e);
                return Err(MarketMakerError::ErrorDepositing);
            }
        }
    }

    deposit_quote_token(
        owner,
        cypher_user_pubkey,
        cypher_group,
        rpc_client,
        amount_rem,
    )
    .await
}

async fn request_airdrop(
    owner: &Keypair,
    rpc_client: Arc<RpcClient>,
) -> Result<(), MarketMakerError> {
    let token_account = derive_quote_token_address(owner.pubkey());
    let airdrop_ix = get_request_airdrop_ix(&token_account, 10_000_000_000);

    let mut builder = FastTxnBuilder::new();

    let token_account_res = get_token_account(Arc::clone(&rpc_client), &token_account).await;
    match token_account_res {
        Ok(_) => (),
        Err(_) => {
            info!(
                "Quote token account does not exist, creating account with key: {} for mint {}.",
                token_account,
                quote_mint::ID
            );
            builder.add(create_associated_token_account(
                &owner.pubkey(),
                &owner.pubkey(),
                &quote_mint::ID,
            ));
        }
    }
    builder.add(airdrop_ix);

    let hash = rpc_client.get_latest_blockhash().await.unwrap();
    let tx = builder.build(hash, owner, None);
    let res = rpc_client
        .send_and_confirm_transaction_with_spinner(&tx)
        .await;
    match res {
        Ok(s) => {
            info!(
                "Successfully requested airdrop. Transaction signature: {}",
                s.to_string()
            );
            Ok(())
        }
        Err(e) => {
            warn!("There was an error requesting airdrop: {}", e.to_string());
            Err(MarketMakerError::ErrorDepositing)
        }
    }
}

async fn deposit_quote_token(
    owner: &Keypair,
    cypher_user_pubkey: &Pubkey,
    cypher_group: &CypherGroup,
    rpc_client: Arc<RpcClient>,
    amount: Number,
) -> Result<(), MarketMakerError> {
    let source_ata = derive_quote_token_address(owner.pubkey());

    let ix = get_deposit_collateral_ix(
        &cypher_group.self_address,
        cypher_user_pubkey,
        &cypher_group.quote_vault(),
        &source_ata,
        &owner.pubkey(),
        amount.as_u64(0),
    );
    let mut builder = FastTxnBuilder::new();
    builder.add(ix);
    let hash = rpc_client.get_latest_blockhash().await.unwrap();
    let tx = builder.build(hash, owner, None);
    let res = rpc_client
        .send_and_confirm_transaction_with_spinner(&tx)
        .await;

    match res {
        Ok(s) => {
            info!(
                "Successfully deposited funds into cypher account. Transaction signature: {}",
                s.to_string()
            );
            Ok(())
        }
        Err(e) => {
            warn!(
                "There was an error depositing funds into cypher account: {}",
                e.to_string()
            );
            Err(MarketMakerError::ErrorDepositing)
        }
    }
}

async fn _fetch_cypher_user(
    cypher_user_pubkey: &Pubkey,
    rpc_client: Arc<RpcClient>,
) -> Result<Box<CypherUser>, MarketMakerError> {
    let res = rpc_client
        .get_account_with_commitment(cypher_user_pubkey, CommitmentConfig::confirmed())
        .await
        .unwrap()
        .value;

    if res.is_some() {
        let account_state = get_zero_copy_account::<CypherUser>(&res.unwrap());
        info!("Successfully fetched cypher account.");
        return Ok(account_state);
    }

    Err(MarketMakerError::ErrorFetchingCypherAccount)
}

async fn _fetch_open_orders(
    open_orders: &Pubkey,
    rpc_client: Arc<RpcClient>,
) -> Result<OpenOrders, MarketMakerError> {
    let res = rpc_client
        .get_account_with_commitment(open_orders, CommitmentConfig::confirmed())
        .await
        .unwrap()
        .value;

    if res.is_some() {
        let ooa: OpenOrders = parse_dex_account(res.unwrap().data);
        info!("Successfully fetched open orders account.");
        return Ok(ooa);
    }

    Err(MarketMakerError::ErrorFetchingOpenOrders)
}

async fn _init_cypher_user(
    cypher_group_pubkey: &Pubkey,
    owner: &Keypair,
    rpc_client: Arc<RpcClient>,
) -> Result<(), MarketMakerError> {
    let res = init_cypher_user(cypher_group_pubkey, owner, &rpc_client).await;

    match res {
        Ok(_) => (),
        Err(e) => {
            warn!(
                "There was an error creating the cypher account: {}",
                e.to_string()
            );
            return Err(MarketMakerError::ErrorCreatingCypherAccount);
        }
    }

    Ok(())
}

async fn _init_open_orders(
    cypher_group_pubkey: &Pubkey,
    cypher_user_pubkey: &Pubkey,
    cypher_market: &Pubkey,
    open_orders: &Pubkey,
    signer: &Keypair,
    rpc_client: Arc<RpcClient>,
) -> Result<(), MarketMakerError> {
    let ix = get_init_open_orders_ix(
        cypher_group_pubkey,
        cypher_user_pubkey,
        cypher_market,
        open_orders,
        &signer.pubkey(),
    );

    let mut builder = FastTxnBuilder::new();
    builder.add(ix);
    let hash = rpc_client.get_latest_blockhash().await.unwrap();
    let tx = builder.build(hash, signer, None);
    let res = rpc_client
        .send_and_confirm_transaction_with_spinner(&tx)
        .await;
    match res {
        Ok(s) => {
            info!(
                "Successfully created open orders account. Transaction signature: {}",
                s.to_string()
            );
            Ok(())
        }
        Err(e) => {
            warn!(
                "There was an error creating the open orders account: {}",
                e.to_string()
            );
            Err(MarketMakerError::ErrorCreatingOpenOrders)
        }
    }
}
