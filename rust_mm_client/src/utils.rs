use anchor_spl::{associated_token, token::spl_token};
use cypher::{
    client::{deposit_collateral_ix, init_cypher_user_ix, init_open_orders_ix},
    quote_mint,
    utils::{derive_cypher_user_address, derive_dex_market_authority},
};
use solana_account_decoder::parse_token::UiTokenAmount;
use solana_client::{client_error::ClientError, nonblocking::rpc_client::RpcClient};
use solana_sdk::{
    commitment_config::CommitmentConfig, instruction::Instruction, pubkey::Pubkey,
    signature::Keypair, signer::Signer,
};
use std::sync::Arc;

use crate::fast_tx_builder::FastTxnBuilder;

pub fn derive_quote_token_address(wallet_address: Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[
            &wallet_address.to_bytes(),
            &spl_token::id().to_bytes(),
            &quote_mint::ID.to_bytes(),
        ],
        &associated_token::ID,
    )
    .0
}

pub async fn get_token_account(
    client: Arc<RpcClient>,
    token_account: &Pubkey,
) -> Result<UiTokenAmount, ClientError> {
    let ta_res = client
        .get_token_account_balance_with_commitment(token_account, CommitmentConfig::confirmed())
        .await;

    let ta = match ta_res {
        Ok(ta) => ta.value,
        Err(e) => {
            return Err(e);
        }
    };

    Ok(ta)
}

pub async fn init_cypher_user(
    group_address: &Pubkey,
    owner: &Keypair,
    rpc: &Arc<RpcClient>,
) -> Result<(), ClientError> {
    let (address, bump) = derive_cypher_user_address(group_address, &owner.pubkey());

    let ix = init_cypher_user_ix(group_address, &address, &owner.pubkey(), bump);

    let mut builder = FastTxnBuilder::new();
    builder.add(ix);
    let hash = rpc.get_latest_blockhash().await?;
    let tx = builder.build(hash, owner, None);
    rpc.send_and_confirm_transaction_with_spinner(&tx)
        .await
        .unwrap();
    Ok(())
}

pub fn get_deposit_collateral_ix(
    cypher_group_pubkey: &Pubkey,
    cypher_user_pubkey: &Pubkey,
    cypher_pc_vault: &Pubkey,
    source_token_account: &Pubkey,
    signer: &Pubkey,
    amount: u64,
) -> Instruction {
    deposit_collateral_ix(
        cypher_group_pubkey,
        cypher_user_pubkey,
        cypher_pc_vault,
        signer,
        source_token_account,
        amount,
    )
}

pub fn get_init_open_orders_ix(
    cypher_group_pubkey: &Pubkey,
    cypher_user_pubkey: &Pubkey,
    cypher_market: &Pubkey,
    open_orders: &Pubkey,
    signer: &Pubkey,
) -> Instruction {
    let market_authority = derive_dex_market_authority(cypher_market);
    init_open_orders_ix(
        cypher_group_pubkey,
        cypher_user_pubkey,
        signer,
        cypher_market,
        open_orders,
        &market_authority,
    )
}
