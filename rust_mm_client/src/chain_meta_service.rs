use std::sync::Arc;
use tokio::sync::RwLock;

use solana_client::{nonblocking::rpc_client::RpcClient, client_error::ClientError};
use solana_sdk::{ hash::Hash};
use tokio::time::Duration;
use solana_sdk::commitment_config::CommitmentConfig;

pub struct ChainMetaService{
    client : Arc<RpcClient>,
    recent_blockhash : RwLock<Hash>
}

//Abstraction here is bad with the exposed Arcs -- maybe fix later
impl ChainMetaService{
    pub async fn new_load(client : Arc<RpcClient>) -> Arc<ChainMetaService>{
        let start_hash = client.get_latest_blockhash().await.unwrap();
        Arc::new(ChainMetaService{
                    client,
                    recent_blockhash : RwLock::new(start_hash)
                })
    }
    #[inline(always)]
    async fn update_chain_meta(self : &Arc<Self>) -> Result<(), ClientError>{
        let hash = self.client.get_latest_blockhash_with_commitment(CommitmentConfig::processed()).await?;
        let slot = self.client.get_slot().await?;
        println!("recent slot {}", slot);
        *self.recent_blockhash.write().await = hash.0;
        Ok(())
    }
    #[inline(always)]
    async fn update_chain_meta_replay(self : Arc<Self>, s_update : u64, milis_update : u32){
        loop {
            if let Err(r) = self.update_chain_meta().await{
                println!("Couldn't get new chain meta!! Error : {}", r);
            }
            tokio::time::sleep(Duration::new(s_update, milis_update)).await;
        }
    }
    #[inline(always)]
    pub async fn start_service(self : &Arc<Self>, s_update : u64,  milis_update : u32){
        let aself = self.clone();
        tokio::spawn(aself.update_chain_meta_replay(s_update, milis_update));
    }
    #[inline(always)]
    pub async fn get_latest_blockhash(&self) -> Hash{
        //Copy and return hash
        *self.recent_blockhash.read().await
    }
}