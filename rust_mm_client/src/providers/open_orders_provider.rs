use cypher::client::parse_dex_account;
use log::{info, warn};
use serum_dex::state::OpenOrders;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use tokio::sync::{
    broadcast::{channel, Receiver, Sender},
    Mutex,
};

use crate::accounts_cache::AccountsCache;

pub struct OpenOrdersProvider {
    cache: Arc<AccountsCache>,
    sender: Arc<Sender<OpenOrders>>,
    receiver: Mutex<Receiver<Pubkey>>,
    shutdown_receiver: Mutex<Receiver<bool>>,
    open_orders_pubkey: Pubkey,
}

impl OpenOrdersProvider {
    pub fn default() -> Self {
        Self {
            cache: Arc::new(AccountsCache::default()),
            sender: Arc::new(channel::<OpenOrders>(u16::MAX as usize).0),
            receiver: Mutex::new(channel::<Pubkey>(u16::MAX as usize).1),
            shutdown_receiver: Mutex::new(channel::<bool>(1).1),
            open_orders_pubkey: Pubkey::default(),
        }
    }

    pub fn new(
        cache: Arc<AccountsCache>,
        sender: Arc<Sender<OpenOrders>>,
        receiver: Receiver<Pubkey>,
        shutdown_receiver: Receiver<bool>,
        open_orders_pubkey: Pubkey,
    ) -> Self {
        Self {
            cache,
            sender,
            receiver: Mutex::new(receiver),
            shutdown_receiver: Mutex::new(shutdown_receiver),
            open_orders_pubkey,
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
                        warn!("[OOAP] There was an error while processing a provider update, restarting loop.");
                        continue;
                    } else {
                        let res = self.process_updates(key.unwrap()).await;
                        match res {
                            Ok(_) => (),
                            Err(_) => {
                                warn!(
                                    "[OOAP] There was an error sending an update about the open orders account with key: {}.",
                                    self.open_orders_pubkey
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
                info!("[OOAP] Received shutdown signal, stopping.",);
                break;
            }
        }
    }

    async fn process_updates(&self, key: Pubkey) -> Result<(), OpenOrdersProviderError> {
        if key == self.open_orders_pubkey {
            let ai = self.cache.get(&key).unwrap();

            let dex_open_orders: OpenOrders = parse_dex_account(ai.account.data.to_vec());

            match self.sender.send(dex_open_orders) {
                Ok(_) => {
                    //info!("[OOAP] Latest price for {}: {}", self.symbol, price);
                    return Ok(());
                }
                Err(_) => {
                    warn!(
                        "[OOAP] Failed to send message about the open orders account with key: {}.",
                        self.open_orders_pubkey
                    );
                    return Err(OpenOrdersProviderError::ChannelSendError);
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, PartialEq)]
pub enum OpenOrdersProviderError {
    ChannelSendError,
}
