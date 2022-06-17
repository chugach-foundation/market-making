use {
    dashmap::{
        mapref::one::Ref,
        DashMap
    },
    tokio::sync::broadcast::{
        Sender, channel
    },
    solana_sdk::pubkey::Pubkey,
    log::warn,
    solana_sdk::account::Account,
    dashmap::mapref::one::RefMut
};

pub struct AccountsCache {
    map: DashMap<Pubkey, AccountState>,
    sender: Sender<Pubkey>,
}

#[derive(Debug)]
pub struct AccountState {
    pub account: Account,
    pub slot: u64
}

impl AccountsCache {
    pub fn default() -> Self {
        Self {
            map: DashMap::default(),
            sender: channel::<Pubkey>(u16::MAX as usize).0
        }
    }

    pub fn new(sender: Sender<Pubkey>) -> Self {
        AccountsCache {
            map: DashMap::new(),
            sender,
        }
    }
    
    pub fn get(&self, key: &Pubkey) -> Option<Ref<'_, Pubkey, AccountState>> {
        self.map.get(key)
    }

    pub fn get_mut(&self, key: &Pubkey) -> Option<RefMut<'_, Pubkey, AccountState>> {
        self.map.get_mut(key)
    }

    pub fn insert(
        &self,
        key: Pubkey,
        data: AccountState,
    ) -> Result<(), AccountsCacheError> {
        //info!("[CACHE] Updating entry for account {}", key.to_string());
        self.map.insert(key, data);

        match self.sender.send(key) {
            Ok(_) => {
                //info!("Updated account with key: {}", key);
                Ok(())
            },
            Err(_) => {
                warn!("Failed to send message about updated account {}", key.to_string());
                Err(AccountsCacheError::ChannelSendError)
            },
        }
    }

}

pub enum AccountsCacheError {
    ChannelSendError,
}