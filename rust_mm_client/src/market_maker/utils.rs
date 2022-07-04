use anchor_lang::ToAccountMetas;
use bytemuck::bytes_of;
use cypher::{
    constants::B_DEX_MARKET_AUTHORITY,
    states::{CypherGroup, CypherMarket, CypherToken},
};
use cypher_tester::{dex, ToPubkey};
use serum_dex::{
    instruction::{CancelOrderInstructionV2, MarketInstruction, NewOrderInstructionV3},
    matching::OrderType,
    state::MarketStateV2,
};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    signature::Keypair,
    signer::Signer,
    sysvar::SysvarId,
};
use std::convert::identity;

bitflags::bitflags! {
    pub struct ClientOrderFlags: u32 {
        const MINTING = 1 << 0;
        const POST_ONLY = 1 << 1;
        const POST_ALLOWED = 1 << 2;
    }
}

pub fn gen_client_order_id(
    orig_client_order_id: u32,
    is_minting_order: bool,
    order_type: OrderType,
) -> u64 {
    let mut client_order_flags = ClientOrderFlags::empty();
    if is_minting_order {
        client_order_flags |= ClientOrderFlags::MINTING;
    }
    match order_type {
        OrderType::Limit => client_order_flags |= ClientOrderFlags::POST_ALLOWED,
        OrderType::PostOnly => {
            client_order_flags |= ClientOrderFlags::POST_ALLOWED;
            client_order_flags |= ClientOrderFlags::POST_ONLY;
        }
        OrderType::ImmediateOrCancel => {}
    }
    let upper = (orig_client_order_id as u64) << 32;
    let lower = client_order_flags.bits() as u64;
    upper | lower
}

pub fn gen_dex_vault_signer_key(
    nonce: u64,
    dex_market_pk: &Pubkey,
) -> Result<Pubkey, ProgramError> {
    let seeds = [dex_market_pk.as_ref(), bytes_of(&nonce)];
    Ok(Pubkey::create_program_address(&seeds, &dex::id())?)
}

pub fn derive_dex_market_authority(dex_market_pk: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[B_DEX_MARKET_AUTHORITY, dex_market_pk.as_ref()],
        &cypher::ID,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn get_cancel_order_ix(
    cypher_group: &CypherGroup,
    cypher_market: &CypherMarket,
    cypher_token: &CypherToken,
    dex_market_state: &MarketStateV2,
    open_orders_pubkey: &Pubkey,
    cypher_user_pubkey: &Pubkey,
    signer: &Keypair,
    ix_data: CancelOrderInstructionV2,
) -> Instruction {
    let accounts = get_cancel_orders_accounts(
        cypher_group,
        cypher_market,
        cypher_token,
        dex_market_state,
        open_orders_pubkey,
        cypher_user_pubkey,
        signer,
    );
    Instruction {
        program_id: cypher::ID,
        accounts,
        data: MarketInstruction::CancelOrderV2(ix_data).pack(),
    }
}

fn get_cancel_orders_accounts(
    cypher_group: &CypherGroup,
    cypher_market: &CypherMarket,
    cypher_token: &CypherToken,
    dex_market_state: &MarketStateV2,
    open_orders_pubkey: &Pubkey,
    cypher_user_pubkey: &Pubkey,
    signer: &Keypair,
) -> Vec<AccountMeta> {
    let dex_vault_signer = gen_dex_vault_signer_key(
        dex_market_state.vault_signer_nonce,
        &cypher_market.dex_market,
    )
    .unwrap();
    let prune_authority = derive_dex_market_authority(&cypher_market.dex_market).0;
    vec![
        AccountMeta::new(cypher_group.self_address, false),
        AccountMeta::new_readonly(cypher_group.vault_signer, false),
        AccountMeta::new(*cypher_user_pubkey, false),
        AccountMeta::new_readonly(signer.pubkey(), true),
        AccountMeta::new(cypher_token.mint, false),
        AccountMeta::new(cypher_token.vault, false),
        AccountMeta::new(cypher_group.quote_vault(), false),
        AccountMeta::new(cypher_market.dex_market, false),
        AccountMeta::new_readonly(prune_authority, false),
        AccountMeta::new(identity(dex_market_state.bids).to_pubkey(), false),
        AccountMeta::new(identity(dex_market_state.asks).to_pubkey(), false),
        AccountMeta::new(*open_orders_pubkey, false),
        AccountMeta::new(identity(dex_market_state.event_q).to_pubkey(), false),
        AccountMeta::new(identity(dex_market_state.coin_vault).to_pubkey(), false),
        AccountMeta::new(identity(dex_market_state.pc_vault).to_pubkey(), false),
        AccountMeta::new_readonly(dex_vault_signer, false),
        AccountMeta::new_readonly(spl_token::id(), false),
        AccountMeta::new_readonly(dex::id(), false),
    ]
}

#[allow(clippy::too_many_arguments)]
pub fn get_new_order_ix(
    cypher_group: &CypherGroup,
    cypher_market: &CypherMarket,
    cypher_token: &CypherToken,
    dex_market_state: &MarketStateV2,
    open_orders_pubkey: &Pubkey,
    cypher_user_pubkey: &Pubkey,
    signer: &Keypair,
    ix_data: NewOrderInstructionV3,
) -> Instruction {
    let accounts = get_new_order_v3_accounts(
        cypher_group,
        cypher_market,
        cypher_token,
        dex_market_state,
        cypher_user_pubkey,
        open_orders_pubkey,
        signer,
    );
    let accounts = accounts.to_account_metas(None);
    Instruction {
        program_id: cypher::ID,
        accounts,
        data: MarketInstruction::NewOrderV3(ix_data).pack(),
    }
}

fn get_new_order_v3_accounts(
    cypher_group: &CypherGroup,
    cypher_market: &CypherMarket,
    cypher_token: &CypherToken,
    dex_market_state: &MarketStateV2,
    cypher_user_pubkey: &Pubkey,
    open_orders_pubkey: &Pubkey,
    signer: &Keypair,
) -> cypher::accounts::NewOrderV3 {
    let dex_vault_signer = gen_dex_vault_signer_key(
        dex_market_state.vault_signer_nonce,
        &cypher_market.dex_market,
    )
    .unwrap();
    cypher::accounts::NewOrderV3 {
        cypher_group: cypher_group.self_address,
        vault_signer: cypher_group.vault_signer,
        price_history: cypher_market.price_history,
        cypher_user: *cypher_user_pubkey,
        user_signer: signer.pubkey(),
        c_asset_mint: cypher_token.mint,
        cypher_c_asset_vault: cypher_token.vault,
        cypher_pc_vault: cypher_group.quote_vault(),
        dex: cypher::accounts::NewOrderV3DexAccounts {
            market: cypher_market.dex_market,
            open_orders: *open_orders_pubkey,
            req_q: identity(dex_market_state.req_q).to_pubkey(),
            event_q: identity(dex_market_state.event_q).to_pubkey(),
            bids: identity(dex_market_state.bids).to_pubkey(),
            asks: identity(dex_market_state.asks).to_pubkey(),
            coin_vault: identity(dex_market_state.coin_vault).to_pubkey(),
            pc_vault: identity(dex_market_state.pc_vault).to_pubkey(),
            vault_signer: dex_vault_signer,
            rent: Rent::id(),
            token_program: spl_token::id(),
            dex_program: dex::id(),
        },
    }
}

#[allow(clippy::too_many_arguments)]
pub fn get_consume_events_ix(
    cypher_group: &CypherGroup,
    cypher_market: &CypherMarket,
    dex_market_state: &MarketStateV2,
    cypher_user_pubkey: &Pubkey,
    open_orders_pubkey: &Pubkey,
    minting_rounds: &Pubkey,
    signer: &Keypair,
    limit: u16,
) -> Instruction {
    let accounts = get_consume_events_accounts(
        cypher_group,
        cypher_market,
        dex_market_state,
        minting_rounds,
        cypher_user_pubkey,
        open_orders_pubkey,
    );
    Instruction {
        program_id: cypher::ID,
        accounts,
        data: MarketInstruction::ConsumeEventsPermissioned(limit).pack(),
    }
}

/// Accounts:
///
/// n = (accounts.len() - 6) / 2;
/// 0                   `[writable]` cypher_group
/// 1                   `[writable]` minting_rounds
/// 2 .. n + 1          `[writable]` cypher_users
/// ================== serum_dex::MarketInstruction::ConsumeEventsPermissioned =====================
/// n + 2 .. n * 2 + 1  `[writable]` corresponding open_orders
/// n * 2 + 2           `[writable]` dex_market
/// n * 2 + 3           `[writable]` event_queue
/// n * 2 + 4           `[]`         crank_authority (needs to be signed by pda)
/// n * 2 + 5           `[]`         dex_program
fn get_consume_events_accounts(
    cypher_group: &CypherGroup,
    cypher_market: &CypherMarket,
    dex_market_state: &MarketStateV2,
    minting_rounds: &Pubkey,
    cypher_user_pubkey: &Pubkey,
    open_orders_pubkey: &Pubkey,
) -> Vec<AccountMeta> {
    let crank_authority = derive_dex_market_authority(&cypher_market.dex_market).0;
    vec![
        AccountMeta::new(cypher_group.self_address, false),
        AccountMeta::new(*minting_rounds, false),
        AccountMeta::new(*cypher_user_pubkey, false),
        AccountMeta::new(*open_orders_pubkey, false),
        AccountMeta::new(cypher_market.dex_market, false),
        AccountMeta::new(identity(dex_market_state.event_q).to_pubkey(), false),
        AccountMeta::new_readonly(crank_authority, false),
        AccountMeta::new_readonly(dex::id(), false),
    ]
}

pub fn get_settle_funds_ix(
    cypher_group: &CypherGroup,
    cypher_market: &CypherMarket,
    cypher_token: &CypherToken,
    dex_market_state: &MarketStateV2,
    cypher_user_pubkey: &Pubkey,
    open_orders_pubkey: &Pubkey,
    signer: &Keypair,
) -> Instruction {
    let accounts = get_settle_funds_accounts(
        cypher_group,
        cypher_market,
        cypher_token,
        dex_market_state,
        cypher_user_pubkey,
        open_orders_pubkey,
        signer,
    );

    Instruction {
        program_id: cypher::ID,
        accounts,
        data: MarketInstruction::SettleFunds.pack(),
    }
}

fn get_settle_funds_accounts(
    cypher_group: &CypherGroup,
    cypher_market: &CypherMarket,
    cypher_token: &CypherToken,
    dex_market_state: &MarketStateV2,
    cypher_user_pubkey: &Pubkey,
    open_orders_pubkey: &Pubkey,
    signer: &Keypair,
) -> Vec<AccountMeta> {
    let dex_vault_signer = gen_dex_vault_signer_key(
        dex_market_state.vault_signer_nonce,
        &cypher_market.dex_market,
    )
    .unwrap();
    vec![
        AccountMeta::new(cypher_group.self_address, false),
        AccountMeta::new_readonly(cypher_group.vault_signer, false),
        AccountMeta::new(*cypher_user_pubkey, false),
        AccountMeta::new_readonly(signer.pubkey(), true),
        AccountMeta::new(cypher_token.mint, false),
        AccountMeta::new(cypher_token.vault, false),
        AccountMeta::new(cypher_group.quote_vault(), false),
        AccountMeta::new(cypher_market.dex_market, false),
        AccountMeta::new(*open_orders_pubkey, false),
        AccountMeta::new(identity(dex_market_state.coin_vault).to_pubkey(), false),
        AccountMeta::new(identity(dex_market_state.pc_vault).to_pubkey(), false),
        AccountMeta::new_readonly(dex_vault_signer, false),
        AccountMeta::new_readonly(spl_token::id(), false),
        AccountMeta::new_readonly(dex::id(), false),
    ]
}
