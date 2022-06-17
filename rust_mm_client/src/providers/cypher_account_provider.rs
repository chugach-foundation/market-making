use std::sync::Arc;
use cypher::states::CypherUser;
use log::{warn, info};
use solana_sdk::{pubkey::Pubkey};
use tokio::sync::{
    broadcast::{Sender, Receiver, channel},
    Mutex
};

use crate::{
    accounts_cache::AccountsCache, utils::get_zero_copy_account
};

pub struct CypherAccountProvider {
    cache: Arc<AccountsCache>,
    sender: Arc<Sender<Box<CypherUser>>>,
    receiver: Mutex<Receiver<Pubkey>>,
    pubkey: Pubkey,
}

impl CypherAccountProvider {
    pub fn default() -> Self {
        Self {
            cache: Arc::new(AccountsCache::default()),
            sender: Arc::new(channel::<Box<CypherUser>>(u16::MAX as usize).0),
            receiver: Mutex::new(channel::<Pubkey>(u16::MAX as usize).1),
            pubkey: Pubkey::default(),
        }
    }

    pub fn new(        
        cache: Arc<AccountsCache>,
        sender: Arc<Sender<Box<CypherUser>>>,
        receiver: Receiver<Pubkey>,
        pubkey: Pubkey,
    ) -> Self {
        Self {
            cache,
            sender,
            receiver: Mutex::new(receiver),
            pubkey
        }
    }

    pub async fn start(
        self: &Arc<Self>
    ) -> Result<(), CypherAccountProviderError> {
        loop {
            match self.process_updates().await {
                Ok(_) => {
                    //info!("[CAP] Oracle provider successfully processed updates, restarting loop.");
                },
                Err(e) => {
                    if e == CypherAccountProviderError::ChannelSendError {
                        warn!("[CAP] There was an error while processing updates, restarting loop.");
                    }
                },
            };
        }
    }

    async fn process_updates(
        &self
    ) -> Result<(), CypherAccountProviderError> {

        let mut receiver = self.receiver.lock().await;

        if let Ok(key) = receiver.recv().await {
            if key == self.pubkey {
                let ai = self.cache.get(&key).unwrap();

                let account_state = get_zero_copy_account::<CypherUser>(&ai.account);

                match self.sender.send(account_state)  {
                    Ok(_) => {
                        return Ok(());
                    }
                    Err(_) => {
                        warn!("[CAP] Failed to send message about cypher account with key {}", self.pubkey);
                        return Err(CypherAccountProviderError::ChannelSendError);
                    },
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, PartialEq)]
pub enum CypherAccountProviderError {
    ChannelSendError

}