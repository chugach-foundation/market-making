use {
    cypher::{
        client::{cancel_order_ix, new_order_v3_ix, ToPubkey},
        utils::{derive_dex_market_authority, gen_dex_vault_signer_key},
        CypherGroup, CypherMarket, CypherToken,
    },
    serum_dex::{
        instruction::{CancelOrderInstructionV2, NewOrderInstructionV3},
        state::MarketStateV2,
    },
    solana_sdk::{instruction::Instruction, pubkey::Pubkey, signature::Keypair, signer::Signer},
    std::convert::identity,
};

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
    let dex_vault_signer = gen_dex_vault_signer_key(
        dex_market_state.vault_signer_nonce,
        &cypher_market.dex_market,
    );
    let prune_authority = derive_dex_market_authority(&cypher_market.dex_market);
    cancel_order_ix(
        &cypher_group.self_address,
        &cypher_group.vault_signer,
        cypher_user_pubkey,
        &signer.pubkey(),
        &cypher_token.mint,
        &cypher_token.vault,
        &cypher_group.quote_vault(),
        &cypher_market.dex_market,
        &prune_authority,
        open_orders_pubkey,
        &identity(dex_market_state.event_q).to_pubkey(),
        &identity(dex_market_state.bids).to_pubkey(),
        &identity(dex_market_state.asks).to_pubkey(),
        &identity(dex_market_state.coin_vault).to_pubkey(),
        &identity(dex_market_state.pc_vault).to_pubkey(),
        &dex_vault_signer,
        ix_data,
    )
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
    let dex_vault_signer = gen_dex_vault_signer_key(
        dex_market_state.vault_signer_nonce,
        &cypher_market.dex_market,
    );
    new_order_v3_ix(
        &cypher_group.self_address,
        &cypher_group.vault_signer,
        &cypher_market.price_history,
        cypher_user_pubkey,
        &signer.pubkey(),
        &cypher_token.mint,
        &cypher_token.vault,
        &cypher_group.quote_vault(),
        &cypher_market.dex_market,
        open_orders_pubkey,
        &identity(dex_market_state.req_q).to_pubkey(),
        &identity(dex_market_state.event_q).to_pubkey(),
        &identity(dex_market_state.bids).to_pubkey(),
        &identity(dex_market_state.asks).to_pubkey(),
        &identity(dex_market_state.coin_vault).to_pubkey(),
        &identity(dex_market_state.pc_vault).to_pubkey(),
        &dex_vault_signer,
        ix_data,
    )
}
