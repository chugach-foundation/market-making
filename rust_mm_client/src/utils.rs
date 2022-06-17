use std::sync::Arc;

use anchor_lang::{ZeroCopy, Owner};
use arrayref::array_ref;
use bytemuck::checked::from_bytes;
use cypher::{
    quote_mint,
    constants::{B_OPEN_ORDERS, B_CYPHER_USER}
};
use cypher_tester::{associated_token, get_request_builder, dex};
use serum_dex::instruction::MarketInstruction;
use solana_client::{nonblocking::rpc_client::RpcClient, client_error::ClientError};
use solana_sdk::{
    pubkey::Pubkey,
    instruction::{Instruction, AccountMeta},
    rent::Rent,
    system_program,
    sysvar::SysvarId, account::Account, signature::Keypair, signer::Signer,
};

use crate::{market_maker::derive_dex_market_authority, fast_tx_builder::FastTxnBuilder};

pub fn get_zero_copy_account<T: ZeroCopy + Owner>(solana_account: &Account) -> Box<T> {
    let data = &solana_account.data.as_slice();
    let disc_bytes = array_ref![data, 0, 8];
    assert_eq!(disc_bytes, &T::discriminator());
    Box::new(*from_bytes::<T>(&data[8..std::mem::size_of::<T>() + 8]))
}

pub fn derive_cypher_user_address(group_address: &Pubkey, owner: &Pubkey) -> (Pubkey, u8) {
    let (address, bump) = Pubkey::find_program_address(
        &[
            B_CYPHER_USER,
            group_address.as_ref(),
            &owner.to_bytes(),
        ],
        &cypher::ID,
    );

    (address, bump)
}

pub fn derive_open_orders_address(dex_market_pk: &Pubkey, cypher_user_pk: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            B_OPEN_ORDERS,
            dex_market_pk.as_ref(),
            cypher_user_pk.as_ref(),
        ],
        &cypher::ID,
    )
}

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

pub async fn init_cypher_user(
    group_address: &Pubkey,
    owner: &Keypair,
    rpc: &Arc<RpcClient>,
) -> Result<(), ClientError> {
    let (address, bump) = derive_cypher_user_address(group_address, &owner.pubkey());

    let ixs = get_request_builder()
        .accounts(cypher::accounts::InitCypherUser {
            cypher_group: *group_address,
            cypher_user: address,
            owner: owner.pubkey(),
            system_program: system_program::id(),
        })
        .args(cypher::instruction::InitCypherUser { bump })
        .instructions()
        .unwrap();

    let mut builder = FastTxnBuilder::new();
    for ix in ixs {
        builder.add(ix);
    }
    let hash = rpc.get_latest_blockhash().await?;
    let tx = builder.build(hash, owner, None);
    rpc.send_and_confirm_transaction_with_spinner(&tx).await?;
    Ok(())
}

pub fn get_deposit_collateral_ix(
    cypher_group_pubkey: &Pubkey,
    cypher_user_pubkey: &Pubkey,
    cypher_pc_vault: &Pubkey,
    source_token_account: &Pubkey,
    signer: &Pubkey,
    amount: u64
) -> Vec<Instruction> {
    let ixs = get_request_builder()
        .accounts(cypher::accounts::DepositCollateral {
            cypher_group: *cypher_group_pubkey,
            cypher_user: *cypher_user_pubkey,
            cypher_pc_vault: *cypher_pc_vault,
            deposit_from: *source_token_account,
            user_signer: *signer,
            token_program: spl_token::ID,
        })
        .args(cypher::instruction::DepositCollateral { amount })
        .instructions()
        .unwrap();
    ixs
}

pub fn get_init_open_orders_ix(
    cypher_group_pubkey: &Pubkey,
    cypher_user_pubkey: &Pubkey,
    cypher_market: &Pubkey,
    open_orders: &Pubkey,
    signer: &Pubkey,
) -> Vec<Instruction> {
    let market_authority = derive_dex_market_authority(cypher_market).0;
    let data = MarketInstruction::InitOpenOrders.pack();
    let accounts: Vec<AccountMeta> = vec![
        AccountMeta::new(*cypher_group_pubkey, false),
        AccountMeta::new(*cypher_user_pubkey, false),
        AccountMeta::new(*signer, true),
        AccountMeta::new_readonly(*cypher_market, false),
        AccountMeta::new_readonly(market_authority, false),
        AccountMeta::new(*open_orders, false),
        AccountMeta::new_readonly(Rent::id(), false),
        AccountMeta::new_readonly(system_program::id(), false),
        AccountMeta::new_readonly(dex::id(), false),
    ];

    vec![Instruction {
        accounts,
        data,
        program_id: cypher::ID
    }]
}