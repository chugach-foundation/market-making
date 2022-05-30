use std::{sync::{Arc}, collections::HashMap, io::Read, thread::AccessError};

use solana_client::{nonblocking::rpc_client::RpcClient, client_error::ClientError};
use solana_sdk::{pubkey::Pubkey, account::Account};
use tokio::{task, sync::{RwLock, RwLockReadGuard}};


pub struct AccountInfoService{
    infos : RwLock<HashMap<Pubkey, Account>>,
    client : Arc<RpcClient>,
    keys : Vec<Pubkey>
}

impl AccountInfoService{
    pub fn new(client : Arc<RpcClient>, keys : &[Pubkey]) -> AccountInfoService{
        let service = AccountInfoService {
            infos : RwLock::new(HashMap::new()),
            client,
            keys : Vec::from(keys)
        };
        service
    }

    //Obtains a read lock to the info service and returns. While this read lock is held, account infos WILL NOT update. Drop this lock to resume updating of the accounts
    #[inline(always)]
    pub async fn get_account_map_read_lock<'a>(&'a self) -> RwLockReadGuard<'a, HashMap<Pubkey, Account>>{
        self.infos.read().await
    }

    pub async fn start_service(self : &Arc<Self>){
        for i in (0..self.keys.len()).step_by(100){
            let aself = self.clone();
            aself.update_infos(i, self.keys.len().min(i+100)).await.unwrap();
            task::spawn(aself.update_infos_replay(i, self.keys.len().min(i+100)));
        }
    }
    #[inline(always)]
    async fn update_infos(self : &Arc<Self>, i1 : usize, i2 : usize) -> Result<(), ClientError> {
            let account_keys = &self.keys[i1..i2];
            let mut infos = self.client.get_multiple_accounts(&account_keys).await?;
            let mut map = self.infos.write().await;
            while !infos.is_empty() {
                let next = infos.pop().unwrap();
                let i = infos.len();
                if let Some(info) = next{
                    let key = account_keys[i];  
                    if map.contains_key(&key){
                        *map.get_mut(&key).unwrap() = info;
                    }
                    else{
                        map.insert(key, info);
                    }                        
                }
                else{
                    println!("An Account info was missing!!");
                }
            }
            Ok(())  
    }
    #[inline(always)]
    pub async fn update_infos_replay(self : Arc<Self>, i1 : usize, i2 : usize){
        loop {
            let res = self.update_infos(i1, i2).await;
            //Add delay here if needed
            if res.is_err(){
                println!("FAILED TO GET NEW ACCOUNT INFOS!!");
            }
        }
    }
}