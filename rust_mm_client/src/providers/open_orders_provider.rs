use std::sync::Arc;
use cypher_tester::parse_dex_account;
use log::warn;
use serum_dex::state::OpenOrders;
use solana_sdk::pubkey::Pubkey;
use tokio::sync::{
    broadcast::{
        Sender, Receiver, channel
    },
    Mutex
};

use crate::accounts_cache::AccountsCache;

pub struct OpenOrdersProvider {
    cache: Arc<AccountsCache>,
    sender: Arc<Sender<OpenOrders>>,
    receiver: Mutex<Receiver<Pubkey>>,
    open_orders_pubkey: Pubkey
}

impl OpenOrdersProvider {
    pub fn default() -> Self {
        Self {
            cache: Arc::new(AccountsCache::default()),
            sender: Arc::new(channel::<OpenOrders>(u16::MAX as usize).0),
            receiver: Mutex::new(channel::<Pubkey>(u16::MAX as usize).1),
            open_orders_pubkey: Pubkey::default(),
        }
    }

    pub fn new(
        cache: Arc<AccountsCache>,
        sender: Arc<Sender<OpenOrders>>,
        receiver: Receiver<Pubkey>,
        open_orders_pubkey: Pubkey
    ) -> Self {
        Self {
            cache,
            sender,
            receiver: Mutex::new(receiver),
            open_orders_pubkey
        }
    }

    pub async fn start(
        self: &Arc<Self>
    ) -> Result<(), OpenOrdersProviderError> {
        loop {
            match self.process_updates().await {
                Ok(_) => {
                    //warn!("[OOAP] Oracle provider successfully processed updates, restarting loop.");
                },
                Err(e) => {
                    if e == OpenOrdersProviderError::ChannelSendError {
                        warn!("[OOAP] There was an error while processing updates, restarting loop.");
                    }
                },
            };
        }
    }

    async fn process_updates(&self) -> Result<(), OpenOrdersProviderError> {
        let mut receiver =  self.receiver.lock().await;

        if let Ok(key) = receiver.recv().await {
            if key == self.open_orders_pubkey {
                let ai = self.cache.get(&key).unwrap();

                let dex_open_orders: OpenOrders = parse_dex_account(ai.account.data.to_vec());

                match self.sender.send(dex_open_orders)  {
                    Ok(_) => {
                        //info!("[OOAP] Latest price for {}: {}", self.symbol, price);
                        return Ok(());
                    }
                    Err(_) => {
                        warn!("[OOAP] Failed to send message about cypher account with key {}", self.open_orders_pubkey);
                        return Err(OpenOrdersProviderError::ChannelSendError);
                    },
                }
            }
        }
        
        Ok(())
    }
}

#[derive(Debug, PartialEq)]
pub enum OpenOrdersProviderError {
    ChannelSendError
}