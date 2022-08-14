use std::sync::Arc;
use futures::StreamExt;
use log::{warn, info};
use solana_account_decoder::UiAccountEncoding;
use solana_client::{nonblocking::{pubsub_client::{PubsubClient, PubsubClientError}, rpc_client::RpcClient}, rpc_config::RpcAccountInfoConfig, client_error::ClientError};
use solana_sdk::{pubkey::Pubkey, commitment_config::CommitmentConfig};
use tokio::{sync::broadcast::{Sender, channel}, task::JoinHandle};

use crate::{accounts_cache::{AccountsCache, AccountState}, providers::get_account_info};

#[derive(Clone, Copy)]
pub struct AccountSubscription {
    pub key: Pubkey,
    pub account_type: Option<u8>,
}

pub struct AccountInfoService {
    cache: Arc<AccountsCache>,
    pubsub_client: Arc<PubsubClient>,
    rpc_client: Arc<RpcClient>,
    subs: Vec<Pubkey>,
    tasks: Vec<JoinHandle<()>>,
    shutdown: Arc<Sender<bool>>,
}

impl AccountInfoService {
    pub async fn default() -> Self {
        Self {
            cache: Arc::new(AccountsCache::default()),
            pubsub_client: Arc::new(PubsubClient::new("wss://devnet.genesysgo.net").await.unwrap()),
            rpc_client: Arc::new(RpcClient::new("http://localhost:8899".to_string())),
            subs: Vec::new(),
            tasks: Vec::new(),
            shutdown: Arc::new(channel::<bool>(1).0)
        }
    }

    pub fn new(
        cache: Arc<AccountsCache>,
        pubsub_client: Arc<PubsubClient>,
        rpc_client: Arc<RpcClient>,
        subs: &[Pubkey],
        shutdown: Arc<Sender<bool>>,
    ) -> Self {
        Self {
            cache,
            pubsub_client,
            rpc_client,
            shutdown,
            subs: Vec::from(subs),
            tasks: Vec::new()
        }
    }

    pub async fn start_service(
        mut self
    ) {
        match self.get_account_infos().await {
            Ok(()) => (),
            Err(e) => {
                warn!("[AIS] There was an error while fetching initial account infos: {}", e.to_string());
            }
        }

        for account_sub in self.subs {
            let handler = SubscriptionHandler::new(
                Arc::clone(&self.cache),
                Arc::clone(&self.pubsub_client),
                Arc::clone(&self.shutdown),
                account_sub,
            );

            let t = tokio::spawn(
                async move {
                    match handler.run().await {
                        Ok(_) => (),
                        Err(e) => {
                            warn!("[AIS] There was an error running subscription handler for account {}: {}", account_sub, e.to_string());
                        }
                    }
                }
            );
            self.tasks.push(t);
        }
        
        for task in self.tasks {
            let res = tokio::join!(task);

            match res {
                (Ok(_),) => (),
                (Err(e),) => {
                    warn!("[AIS] There was an error joining with task: {}", e.to_string());
                }
            };
        }
    }

    #[inline(always)]
    async fn get_account_infos(&self) -> Result<(), ClientError> {
        let rpc_result = self
            .rpc_client
            .get_multiple_accounts_with_commitment(&self.subs, CommitmentConfig::confirmed())
            .await;

        let res = match rpc_result {
            Ok(r) => r,
            Err(e) => {
                warn!("[AIS] Could not fetch account infos: {}", e.to_string());
                return Err(e);
            }
        };

        let mut infos = res.value;
        info!("[AIS] Fetched {} account infos.", infos.len());

        while !infos.is_empty() {
            let next = infos.pop().unwrap();
            let i = infos.len();
            let key = self.subs[i];
            //info!("[AIS] [{}/{}] Fetched account {}", i, infos.len(), key.to_string());

            let info = match next {
                Some(ai) => ai,
                None => {
                    warn!(
                        "[AIS] [{}/{}] An account info was missing!!",
                        i,
                        infos.len()
                    );
                    continue;
                }
            };
            //info!("[AIS] [{}/{}] Account {} has data: {}", i, infos.len(), key.to_string(), base64::encode(&info.data));
            let res = self.cache.insert(
                key,
                AccountState {
                    account: info.data,
                    slot: res.context.slot,
                },
            );

            match res {
                Ok(_) => (),
                Err(_) => {
                    warn!("[AIS] There was an error while inserting account info in the cache.");
                }
            };
        }

        Ok(())
    }

}

struct SubscriptionHandler {
    cache: Arc<AccountsCache>,
    pubsub_client: Arc<PubsubClient>,
    shutdown: Arc<Sender<bool>>,
    sub: Pubkey,
}

impl SubscriptionHandler {
    pub fn new(
        cache: Arc<AccountsCache>,
        pubsub_client: Arc<PubsubClient>,
        shutdown: Arc<Sender<bool>>,
        sub: Pubkey,
    ) -> Self {
        Self {
            cache,
            pubsub_client,
            shutdown,
            sub
        }
    }

    pub async fn run(
        self
    ) -> Result<(), PubsubClientError> {
        let mut shutdown_receiver = self.shutdown.subscribe();
        let sub = self.pubsub_client
            .account_subscribe(
                &self.sub,
                Some(RpcAccountInfoConfig {
                    commitment: Some(CommitmentConfig::confirmed()),
                    encoding: Some(UiAccountEncoding::Base64),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();

        let mut stream = sub.0;
        loop {
            tokio::select! {
                update = stream.next() => {
                    if update.is_some() {
                        let account_res = update.unwrap();
                        let account_data = get_account_info(&account_res.value).unwrap();
                        info!("[AIS] Received account update for {}, updating cache.", self.sub);
                        let res = self.cache.insert(self.sub, AccountState{
                            account: account_data,
                            slot: account_res.context.slot,
                        });

                        match res {
                            Ok(_) => (),
                            Err(_) => {
                                warn!("[AIS] There was an error while inserting account info in the cache.");
                            }
                        }
                    }
                },
                _ = shutdown_receiver.recv() => {
                    info!("[AIS] Shutting down subscription handler for {}", self.sub);
                    break;
                }
            }
        }
        Ok(())
    }
}