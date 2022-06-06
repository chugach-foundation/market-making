#![allow(dead_code)]
use crate::cypher_group::CypherGroupInfo;
use crate::fast_tx_builder::FastTxnBuilder;
use anchor_lang::{Owner, ToAccountMetas, ZeroCopy};
use arrayref::array_ref;
use bytemuck::checked::from_bytes;
use cypher::states::CypherGroup;
use cypher::{
    constants::B_CYPHER_USER,
    states::{CypherMarket, CypherToken, CypherUser},
};
use cypher_math::Number;
use cypher_tester::*;
use cypher_tester::{derive_open_orders_address, get_request_builder, WalletCookie};
use serum_dex::state::MarketStateV2;
use serum_dex::{
    instruction::{CancelOrderInstructionV2, MarketInstruction, NewOrderInstructionV3},
    state::OpenOrders as DexOpenOrders,
};
use solana_client::client_error::ClientError;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    account::Account,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    rent::Rent,
    signature::Keypair,
    signer::Signer,
    system_program,
    sysvar::SysvarId,
};
use std::convert::identity;
pub const INIT_WALLET_QUOTE_BALANCE: u64 = 10_000_000;

pub struct CypherMarketInfo {
    market_idx: usize,
    market: CypherMarket,
    token: CypherToken,
    state: MarketStateV2,
    ooaddr: Pubkey,
}

pub struct CypherMarketUser {
    pub address: Pubkey,
    pub wallet: WalletCookie,
    pub account_state: Box<CypherUser>,
    pub group: CypherGroupInfo,
    pub market_info: CypherMarketInfo,
}
//TODO -- clean up code
impl CypherMarketUser {
    pub async fn init(
        group_address: Pubkey,
        user_kp: Keypair,
        rpc: &RpcClient,
        market_idx: usize,
    ) -> Result<CypherMarketUser, ClientError> {
        let (address, bump) = Pubkey::find_program_address(
            &[
                B_CYPHER_USER,
                group_address.as_ref(),
                &user_kp.pubkey().to_bytes(),
            ],
            &cypher::ID,
        );
        let ixs = get_request_builder()
            .accounts(cypher::accounts::InitCypherUser {
                cypher_group: group_address,
                cypher_user: address,
                owner: user_kp.pubkey(),
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
        let tx = builder.build(hash, &user_kp, None);
        rpc.send_and_confirm_transaction_with_spinner(&tx).await?;
        let acc = rpc.get_account(&address).await?;
        let account_state = get_zero_copy_account::<CypherUser>(&acc);

        let group_account = rpc.get_account(&group_address).await?;
        let group = get_zero_copy_account::<CypherGroup>(&group_account);
        let group_cookie = CypherGroupInfo {
            address: group_address,
            account_state: group,
        };
        let dex_market = group_cookie.load_dex_market(market_idx, rpc).await;
        let market = *group_cookie.account_state.get_cypher_market(market_idx);
        let market_info = CypherMarketInfo {
            market_idx,
            market: *group_cookie.account_state.get_cypher_market(market_idx),
            token: *group_cookie.account_state.get_cypher_token(market_idx),
            state: dex_market,
            ooaddr: derive_open_orders_address(&market.dex_market, &user_kp.pubkey()).0,
        };
        Ok(CypherMarketUser {
            address,
            wallet: WalletCookie { signer: user_kp },
            account_state,
            group: group_cookie,
            market_info,
        })
    }
    //In the future -- change these to not use vecs. Heap allocations = syscall = slow af
    pub async fn get_deposit_collateral_ix(&self, amount: u64) -> Vec<Instruction> {
        let ixs = get_request_builder()
            .accounts(cypher::accounts::DepositCollateral {
                cypher_group: self.group.address,
                cypher_user: self.address,
                cypher_pc_vault: self.group.account_state.quote_vault(),
                deposit_from: self.wallet.quote_token_pk(),
                user_signer: self.wallet.signer_pk(),
                token_program: spl_token::ID,
            })
            .args(cypher::instruction::DepositCollateral { amount })
            .instructions()
            .unwrap();
        ixs
    }

    pub async fn withdraw_collateral(&self, amount: u64) -> Vec<Instruction> {
        let ixs = get_request_builder()
            .accounts(cypher::accounts::WithdrawCollateral {
                cypher_group: self.group.address,
                vault_signer: self.group.account_state.vault_signer,
                cypher_user: self.address,
                cypher_pc_vault: self.group.account_state.quote_vault(),
                withdraw_to: self.wallet.quote_token_pk(),
                user_signer: self.wallet.signer_pk(),
                token_program: spl_token::id(),
            })
            .args(cypher::instruction::WithdrawCollateral { amount })
            .instructions()
            .unwrap();
        ixs
    }

    pub async fn deposit_market_collateral(&self, amount: u64) -> Vec<Instruction> {
        let ixs = get_request_builder()
            .accounts(cypher::accounts::DepositMarketCollateral {
                cypher_group: self.group.address,
                cypher_user: self.address,
                user_signer: self.wallet.signer_pk(),
                cypher_pc_vault: self.group.account_state.quote_vault(),
                deposit_from: self.wallet.quote_token_pk(),
                token_program: spl_token::id(),
            })
            .args(cypher::instruction::DepositMarketCollateral {
                c_asset_mint: self.market_info.token.mint,
                amount,
            })
            .instructions()
            .unwrap();
        ixs
    }

    pub async fn withdraw_market_collateral(&self, amount: u64) -> Vec<Instruction> {
        let cypher_token = &self.market_info.token;
        let ixs = get_request_builder()
            .accounts(cypher::accounts::WithdrawMarketCollateral {
                cypher_group: self.group.address,
                vault_signer: self.group.account_state.vault_signer,
                cypher_user: self.address,
                user_signer: self.wallet.signer_pk(),
                cypher_pc_vault: self.group.account_state.quote_vault(),
                withdraw_to: self.wallet.quote_token_pk(),
                token_program: spl_token::id(),
            })
            .args(cypher::instruction::WithdrawMarketCollateral {
                c_asset_mint: cypher_token.mint,
                amount,
            })
            .instructions()
            .unwrap();
        ixs
    }

    pub async fn init_open_orders(&self, rpc: &RpcClient) -> Result<(), ClientError> {
        let cypher_market = &self.market_info.market;
        let dex_market_addr = cypher_market.dex_market;
        let open_orders_addr = self.market_info.ooaddr;
        let dex_market_authority = derive_dex_market_authority(&cypher_market.dex_market).0;
        let data = MarketInstruction::InitOpenOrders.pack();
        let accounts: Vec<AccountMeta> = vec![
            AccountMeta::new(self.group.address, false),
            AccountMeta::new(self.address, false),
            AccountMeta::new(self.wallet.signer_pk(), true),
            AccountMeta::new_readonly(dex_market_addr, false),
            AccountMeta::new_readonly(dex_market_authority, false),
            AccountMeta::new(open_orders_addr, false),
            AccountMeta::new_readonly(Rent::id(), false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(dex::id(), false),
        ];
        let ix = Instruction {
            program_id: cypher::ID,
            data,
            accounts,
        };
        let mut builder = FastTxnBuilder::new();
        builder.add(ix);
        let hash = rpc.get_latest_blockhash().await?;
        let tx = builder.build(hash, &self.wallet.signer, None);
        println!("initializing open orders...");
        rpc.send_and_confirm_transaction_with_spinner(&tx).await?;
        Ok(())
    }

    fn get_new_order_v3_accounts(&self) -> cypher::accounts::NewOrderV3 {
        let cypher_market = self.market_info.market;
        let cypher_token = self.market_info.token;
        let dex_market_state = &self.market_info.state;
        let dex_vault_signer = gen_dex_vault_signer_key(
            dex_market_state.vault_signer_nonce,
            &cypher_market.dex_market,
        )
        .unwrap();
        cypher::accounts::NewOrderV3 {
            cypher_group: self.group.address,
            vault_signer: self.group.account_state.vault_signer,
            price_history: cypher_market.price_history,
            minting_rounds: cypher_market.minting_rounds,
            cypher_user: self.address,
            user_signer: self.wallet.signer_pk(),
            c_asset_mint: cypher_token.mint,
            cypher_c_asset_vault: cypher_token.vault,
            cypher_pc_vault: self.group.account_state.quote_vault(),
            dex: cypher::accounts::NewOrderV3DexAccounts {
                market: cypher_market.dex_market,
                open_orders: self.market_info.ooaddr,
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

    pub async fn mint_and_sell(&self, price: u64, amount: u64) -> Vec<Instruction> {
        let ixs = get_request_builder()
            .accounts(self.get_new_order_v3_accounts())
            .args(cypher::instruction::MintAndSell { price, amount })
            .instructions()
            .unwrap();

        ixs
    }

    pub async fn get_buy_and_burn_ixs(&self, max_repay_amount: u64) -> Vec<Instruction> {
        let ixs = get_request_builder()
            .accounts(self.get_new_order_v3_accounts())
            .args(cypher::instruction::BuyAndBurn { max_repay_amount })
            .instructions()
            .unwrap();

        ixs
    }

    pub async fn new_order_v3(&self, ix_data: NewOrderInstructionV3) -> Instruction {
        let accounts = self.get_new_order_v3_accounts();
        let accounts = accounts.to_account_metas(None);
        let ix = Instruction {
            program_id: cypher::ID,
            accounts,
            data: MarketInstruction::NewOrderV3(ix_data).pack(),
        };
        ix
    }

    fn get_cancel_orders_accounts(
        &self,
        group: &CypherGroupInfo,
        require_signer: bool,
    ) -> Vec<AccountMeta> {
        let cypher_market = self.market_info.market;
        let cypher_token = self.market_info.token;
        let dex_vault_signer = gen_dex_vault_signer_key(
            self.market_info.state.vault_signer_nonce,
            &cypher_market.dex_market,
        )
        .unwrap();
        let prune_authority = derive_dex_market_authority(&cypher_market.dex_market).0;
        let open_orders_addr = self.market_info.ooaddr;
        vec![
            AccountMeta::new(group.address, false),
            AccountMeta::new_readonly(group.account_state.vault_signer, false),
            AccountMeta::new(self.address, false),
            AccountMeta::new_readonly(self.wallet.signer_pk(), require_signer),
            AccountMeta::new(cypher_token.mint, false),
            AccountMeta::new(cypher_token.vault, false),
            AccountMeta::new(group.account_state.quote_vault(), false),
            AccountMeta::new(cypher_market.dex_market, false),
            AccountMeta::new_readonly(prune_authority, false),
            AccountMeta::new(identity(self.market_info.state.bids).to_pubkey(), false),
            AccountMeta::new(identity(self.market_info.state.asks).to_pubkey(), false),
            AccountMeta::new(open_orders_addr, false),
            AccountMeta::new(identity(self.market_info.state.event_q).to_pubkey(), false),
            AccountMeta::new(
                identity(self.market_info.state.coin_vault).to_pubkey(),
                false,
            ),
            AccountMeta::new(identity(self.market_info.state.pc_vault).to_pubkey(), false),
            AccountMeta::new_readonly(dex_vault_signer, false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(dex::id(), false),
        ]
    }

    pub async fn cancel_order_v2(
        &self,
        group: &CypherGroupInfo,
        ix_data: CancelOrderInstructionV2,
    ) -> Instruction {
        let accounts = self.get_cancel_orders_accounts(group, true);
        let ix = Instruction {
            program_id: cypher::ID,
            accounts,
            data: MarketInstruction::CancelOrderV2(ix_data).pack(),
        };
        ix
    }

    pub fn get_settle_funds_ix(&self, group: &mut CypherGroupInfo) -> Instruction {
        let cypher_market = &self.market_info.market;
        let cypher_token = &self.market_info.token;
        let dex_vault_signer = gen_dex_vault_signer_key(
            self.market_info.state.vault_signer_nonce,
            &cypher_market.dex_market,
        )
        .unwrap();
        let open_orders_addr = self.market_info.ooaddr;
        let accounts: Vec<AccountMeta> = vec![
            AccountMeta::new(group.address, false),
            AccountMeta::new_readonly(group.account_state.vault_signer, false),
            AccountMeta::new(self.address, false),
            AccountMeta::new_readonly(self.wallet.signer_pk(), true),
            AccountMeta::new(cypher_token.mint, false),
            AccountMeta::new(cypher_token.vault, false),
            AccountMeta::new(group.account_state.quote_vault(), false),
            AccountMeta::new(cypher_market.dex_market, false),
            AccountMeta::new(open_orders_addr, false),
            AccountMeta::new(
                identity(self.market_info.state.coin_vault).to_pubkey(),
                false,
            ),
            AccountMeta::new(identity(self.market_info.state.pc_vault).to_pubkey(), false),
            AccountMeta::new_readonly(dex_vault_signer, false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(dex::id(), false),
        ];
        let ix = Instruction {
            program_id: cypher::ID,
            accounts,
            data: MarketInstruction::SettleFunds.pack(),
        };
        ix
    }

    pub async fn get_settle_position_ixs(
        group: &CypherGroupInfo,
        user: &CypherMarketUser,
        market_idx: usize,
    ) -> Vec<Instruction> {
        let cypher_market = group.account_state.get_cypher_market(market_idx);
        let cypher_token = group.account_state.get_cypher_token(market_idx);
        let ixs = get_request_builder()
            .accounts(cypher::accounts::SettlePosition {
                cypher_group: group.address,
                vault_signer: group.account_state.vault_signer,
                minting_rounds: cypher_market.minting_rounds,
                cypher_user: user.address,
                c_asset_mint: cypher_token.mint,
                cypher_c_asset_vault: cypher_token.vault,
                cypher_pc_vault: group.account_state.quote_vault(),
                withdraw_to: user.wallet.quote_token_pk(),
                token_program: spl_token::id(),
            })
            .args(cypher::instruction::SettlePosition {})
            .instructions()
            .unwrap();

        ixs
    }

    ///acc : Account data for the CypherUser -
    ///Updates the current state so information is kept up to date. Pass in latest Account data
    pub fn update_account_state(&mut self, acc: &Account) {
        self.account_state = get_zero_copy_account::<CypherUser>(&acc);
    }

    pub fn get_mint_c_ratio(&self, group: &CypherGroupInfo, market_idx: usize) -> Number {
        let cypher_market = group.account_state.get_cypher_market(market_idx);
        self.account_state
            .get_c_asset(market_idx)
            .unwrap()
            .get_mint_c_ratio(&cypher_market)
            .unwrap()
    }

    pub fn get_deposit_position(&self, group: &CypherGroupInfo, token_idx: usize) -> Number {
        let cypher_token = group.account_state.get_cypher_token(token_idx);
        self.account_state
            .get_position(token_idx)
            .unwrap()
            .native_deposits(&cypher_token)
    }

    pub fn get_borrow_position(&self, group: &CypherGroupInfo, token_idx: usize) -> Number {
        let cypher_token = group.account_state.get_cypher_token(token_idx);
        self.account_state
            .get_position(token_idx)
            .unwrap()
            .native_borrows(&cypher_token)
    }

    pub async fn get_orders(&self, dex_oo_acc: Account) -> Vec<CancelOrderInstructionV2> {
        let dex_open_orders: DexOpenOrders = { parse_dex_account(dex_oo_acc.data) };
        struct Iter {
            bits: u128,
        }
        impl Iterator for Iter {
            type Item = u8;
            #[inline(always)]
            fn next(&mut self) -> Option<Self::Item> {
                if self.bits == 0 {
                    None
                } else {
                    let next = self.bits.trailing_zeros();
                    let mask = 1u128 << next;
                    self.bits &= !mask;
                    Some(next as u8)
                }
            }
        }
        let filled_slot_iter = Iter {
            bits: !dex_open_orders.free_slot_bits,
        };
        let mut orders = vec![];
        for slot in filled_slot_iter {
            let order_id = dex_open_orders.orders[slot as usize];
            let side = dex_open_orders.slot_side(slot).unwrap();
            orders.push(CancelOrderInstructionV2 { order_id, side });
        }
        orders
    }

    fn get_open_orders_addr(&self, group: &CypherGroupInfo, market_idx: usize) -> Pubkey {
        let cypher_market = group.account_state.get_cypher_market(market_idx);
        derive_open_orders_address(&cypher_market.dex_market, &self.address).0
    }
}

fn get_zero_copy_account<T: ZeroCopy + Owner>(solana_account: &Account) -> Box<T> {
    let data = &solana_account.data.as_slice();
    let disc_bytes = array_ref![data, 0, 8];
    assert_eq!(disc_bytes, &T::discriminator());
    Box::new(*from_bytes::<T>(&data[8..std::mem::size_of::<T>() + 8]))
}
