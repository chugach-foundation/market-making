use std::sync::Arc;
use log::{info, warn};
use solana_sdk::hash::Hash;
use tokio::{time::Duration, task};
use solana_sdk::commitment_config::CommitmentConfig;
use solana_client::{
    nonblocking::rpc_client::RpcClient,
    client_error::ClientError
};
use tokio::{
    sync::RwLock,
    time::sleep,
};

pub struct ChainMetaService {
    client: Arc<RpcClient>,
    recent_blockhash: RwLock<Hash>,
    slot: RwLock<u64>,
}

impl ChainMetaService {
    pub fn default() -> Self {
        Self { 
            client: Arc::new(RpcClient::new("http://localhost:8899".to_string())),
            recent_blockhash: RwLock::new(Hash::default()),
            slot: RwLock::new(u64::default())
        }
    }

    pub fn new(
        client: Arc<RpcClient>,        
    ) -> ChainMetaService {
        ChainMetaService { 
            client,
            ..ChainMetaService::default()
        }
    }

    #[inline(always)]
    async fn update_chain_meta(self: &Arc<Self>) -> Result<(), ClientError>{
        let hash_res = self.client.get_latest_blockhash_with_commitment(
            CommitmentConfig::confirmed()
        ).await;
        let hash = match hash_res {
            Ok(hash) => hash,
            Err(e) => {
                warn!("[CMS] Failed to fetch recent block hash: {}", e.to_string());
                return Err(e);
            }
        };
        info!("[CMS] Fetched recent block hash: {}", hash.0.to_string());
        *self.recent_blockhash.write().await = hash.0;

        let slot_res = self.client.get_slot().await;
        let slot = match slot_res {
            Ok(slot) => slot,
            Err(e) => {
                warn!("[CMS] Failed to fetch recent slot: {}", e.to_string());
                return Err(e);
            }
        };
        info!("[CMS] Fetched recent slot: {}", slot);
        *self.slot.write().await = slot;

        Ok(())
    }

    #[inline(always)]
    async fn update_chain_meta_replay(
        self: Arc<Self>
    ) {
        loop {
            let res = self.update_chain_meta().await;
            
            if res.is_err() {
                warn!("[CMS] Couldn't get new chain meta! Error: {}", res.err().unwrap().to_string());
            }

            sleep(Duration::from_millis(2500)).await;
        }
    }

    #[inline(always)]
    pub async fn start_service(
        self: &Arc<Self>
    ) -> Result<(), task::JoinError> {
        let aself = self.clone();
        let t = tokio::spawn(aself.update_chain_meta_replay()).await;
        
        if let Err(e) = t {
            warn!("[CMS] Error attempting to join with task: {}", e.to_string());
            return Err(e);
        };

        Ok(())
    }

    #[inline(always)]
    pub async fn get_latest_blockhash(self: &Arc<Self>) -> Hash {
        //Copy and return hash
        *self.recent_blockhash.read().await
    }

    #[inline(always)]
    pub async fn get_latest_slot(&self) -> u64 {
        *self.slot.read().await
    }

}