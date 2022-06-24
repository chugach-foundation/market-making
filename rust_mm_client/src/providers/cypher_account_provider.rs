use cypher::states::CypherUser;
use log::{info, warn};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use tokio::sync::{
    broadcast::{channel, Receiver, Sender},
    Mutex,
};

use crate::{accounts_cache::AccountsCache, utils::get_zero_copy_account, MarketMakerError};

pub struct CypherAccountProvider {
    cache: Arc<AccountsCache>,
    sender: Arc<Sender<Box<CypherUser>>>,
    receiver: Mutex<Receiver<Pubkey>>,
    shutdown_receiver: Mutex<Receiver<bool>>,
    pubkey: Pubkey,
}

impl CypherAccountProvider {
    pub fn default() -> Self {
        Self {
            cache: Arc::new(AccountsCache::default()),
            sender: Arc::new(channel::<Box<CypherUser>>(u16::MAX as usize).0),
            receiver: Mutex::new(channel::<Pubkey>(u16::MAX as usize).1),
            shutdown_receiver: Mutex::new(channel::<bool>(1).1),
            pubkey: Pubkey::default(),
        }
    }

    pub fn new(
        cache: Arc<AccountsCache>,
        sender: Arc<Sender<Box<CypherUser>>>,
        receiver: Receiver<Pubkey>,
        shutdown_receiver: Receiver<bool>,
        pubkey: Pubkey,
    ) -> Self {
        Self {
            cache,
            sender,
            receiver: Mutex::new(receiver),
            shutdown_receiver: Mutex::new(shutdown_receiver),
            pubkey,
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
                        warn!("[CAP] There was an error while processing a provider update, restarting loop.");
                        continue;
                    } else {
                        let res = self.process_updates(key.unwrap()).await;
                        match res {
                            Ok(_) => (),
                            Err(_) => {
                                info!(
                                    "[CAP] There was an error sending an update about the cypher account.",
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
                info!("[CAP] Received shutdown signal, stopping.",);
                break;
            }
        }
    }

    async fn process_updates(&self, key: Pubkey) -> Result<(), MarketMakerError> {
        if key == self.pubkey {
            let ai = self.cache.get(&key).unwrap();

            let account_state = get_zero_copy_account::<CypherUser>(&ai.account);

            match self.sender.send(account_state) {
                Ok(_) => {
                    return Ok(());
                }
                Err(_) => {
                    return Err(MarketMakerError::ChannelSendError);
                }
            }
        }

        Ok(())
    }
}
