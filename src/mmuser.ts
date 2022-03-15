import {
    PublicKey,
    Transaction,
    TransactionInstruction,
    SYSVAR_RENT_PUBKEY,
    AccountMeta,
} from "@solana/web3.js";
import { TOKEN_PROGRAM_ID } from "@solana/spl-token";
import { utils } from "@project-serum/anchor";
import { Order } from "@project-serum/serum/lib/market";
import { CypherClient, CypherUser } from "@chugach-foundation/cypher-client";
import { makeRequestCuInstr, TokenAmount } from "@chugach-foundation/cypher-client/src/utils";
import type { SUBSCRIPTION_STRATEGY, PlaceOrderParams, UI_EVENT } from "@chugach-foundation/cypher-client";
import { PermissionedMarket } from "@chugach-foundation/cypher-client/src/middleware";
import { EventMap } from "@chugach-foundation/cypher-client";
//Literally just a copypaste... Let's talk about an actual solution to creating instructions later...
export class CypherUserController {
    private eventListeners: {[key in UI_EVENT]: number};

    constructor(
        public client: CypherClient,
        public user: CypherUser,
        private associatedUSDCAddress: PublicKey
    ) {
        // @ts-ignore
        this.eventListeners = {};
    }

    static async load(
        client: CypherClient,
        groupAddr: PublicKey,
        _: SUBSCRIPTION_STRATEGY
    ): Promise<CypherUserController> {
        const user = await CypherUser.loadOrCreate(client, groupAddr);
        const associatedUSDCAddress = await client.getAssociatedUSDCAddress();
        const userController = new CypherUserController(
            client,
            user,
            associatedUSDCAddress
        );
        return userController;
    }

    get group() {
        return this.user.group;
    }

    get address() {
        return this.user.address;
    }

    get assetsValue() {
        return this.user.getAssetsValue(this.client);
    }

    get liabsValue() {
        return this.user.getLiabsValue(this.client);
    }

    get cRatio() {
        return this.user.getCRatio(this.client);
    }

    get portfolioValue() {
        return this.user.getPortfolioValue(this.client);
    }

    async getWalletUSDCBalance() {
        return this.client.getWalletUSDCBalance();
    }



    getMarketProxy(cAssetMint: PublicKey) {
        return this.group.getMarketProxy(this.client, cAssetMint);
    }

    getUiOpenOrdersInfo(cAssetMint: PublicKey) {
        return this.user.getUiOpenOrdersInfo(this.client, cAssetMint);
    }

    getExecutedMarkets() {
        return this.user.getExecutedMarkets();
    }

    async getMarketOrders(cAssetMint: PublicKey) {
        return this.user.getMarketOrders(this.client, cAssetMint);
    }

    async getMintingRewardAmount(cAssetMint: PublicKey) {
        return this.user.getMintingRewardAmount(this.client, cAssetMint);
    }

    async needMintingOrdersCancelled(cAsssetMint: PublicKey) {
        return this.user.needMintingOrdersCancelled(this.client, cAsssetMint);
    }

    async needCollateralLiquidationForCirculatingDebt(cAssetMint: PublicKey) {
        return this.user.needCollateralLiquiationForCirculatingDebt(
            this.client,
            cAssetMint
        );
    }

    subscribe() {
        this.group.subscribe(this.client);
        this.user.subscribe(this.client);
    }

    async unsubscribe() {
        await Promise.all([
            this.group.unsubscribe(this.client),
            this.user.unsubscribe(this.client),
        ]);
    }

    subscribeToEvent(eventName: UI_EVENT) {
        const listener = this.client.program.addEventListener(
            EventMap[eventName],
            (event, slot) => {
                console.log(event, slot);
            }
        );
        this.eventListeners[eventName] = listener;
    }

    async unsubscribeFromEvent(eventName: UI_EVENT) {
        await this.client.program.removeEventListener(this.eventListeners[eventName]);
    }

    async faucetUSDC(uiAmount: number) {
        if (!this.client.faucetClient) {
            throw new Error("Can't find faucet client.");
        }
        const tx = new Transaction();
        const [createAccInstr, mintInstr] = await Promise.all([
            this.client.getCreateAssociatedTokenAccountInstr(
                this.client.quoteMint
            ),
            this.client.faucetClient.getMintInstr(
                this.associatedUSDCAddress,
                uiAmount
            ),
        ]);
        if (createAccInstr) {
            tx.add(createAccInstr);
        }
        tx.add(mintInstr);
        return this.client.provider.send(tx);
    }

    private async hasOpenOrders(cAssetMint: PublicKey): Promise<boolean> {
        const marketProxy = this.group.getMarketProxy(this.client, cAssetMint);
        if (
            (
                await marketProxy.market.findOpenOrdersAccountsForOwner(
                    this.client.connection,
                    this.user.address
                )
            ).length
        ) {
            return true;
        } else {
            return false;
        }
    }

    private async makeInitOpenOrdersInstr(
        cAssetMint: PublicKey
    ): Promise<TransactionInstruction | undefined> {
        if (await this.hasOpenOrders(cAssetMint)) {
            return;
        }
        const marketProxy = this.group.getMarketProxy(this.client, cAssetMint);
        return marketProxy.instruction.initOpenOrders(
            this.client.program.provider.wallet.publicKey,
            marketProxy.market.address,
            marketProxy.market.address, // placeholder
            marketProxy.market.address // placeholder
        );
    }

    async depositCollateral(cAssetMint: PublicKey, uiAmount: number) {
        const amount = TokenAmount.toSplUSDCAmount(uiAmount);
        const token = this.group.getCypherToken(this.client, cAssetMint);
        this.group.getCypherMarket(this.client, cAssetMint);
        return this.client.program.rpc.depositMintCollateral(
            token.mint,
            amount,
            {
                accounts: {
                    cypherGroup: this.group.address,
                    cypherUser: this.user.address,
                    depositFrom: this.associatedUSDCAddress,
                    cypherPcVault: this.group.quoteVault,
                    depositAuthority:
                        this.client.program.provider.wallet.publicKey,
                    tokenProgram: TOKEN_PROGRAM_ID,
                },
            }
        );
    }

    async withdrawCollateral(cAssetMint: PublicKey, uiAmount: number) {
        const amount = TokenAmount.toSplUSDCAmount(uiAmount);
        const token = this.group.getCypherToken(this.client, cAssetMint);
        this.group.getCypherMarket(this.client, cAssetMint);
        return this.client.program.rpc.withdrawMintCollateral(
            token.mint,
            amount,
            {
                accounts: {
                    cypherGroup: this.group.address,
                    cypherUser: this.user.address,
                    groupSigner: this.group.state.signerKey,
                    cypherPcVault: this.group.quoteVault,
                    withdrawTo: this.associatedUSDCAddress,
                    userOwner: this.client.program.provider.wallet.publicKey,
                    tokenProgram: TOKEN_PROGRAM_ID,
                },
            }
        );
    }

    makeMintCAssetsInstr(
        cAssetMint: PublicKey,
        uiAmount: number,
        uiPrice: number
    ): TransactionInstruction {
        const amount = TokenAmount.toSplCAssetAmount(uiAmount);
        const price = TokenAmount.toSplCAssetPrice(uiPrice);
        const token = this.group.getCypherToken(this.client, cAssetMint);
        const market = this.group.getCypherMarket(this.client, cAssetMint);
        const marketProxy = this.group.getMarketProxy(this.client, cAssetMint);
        return this.client.program.instruction.mintCAssets(amount, price, {
            accounts: {
                cypherGroup: this.group.address,
                cypherTwap: market.twap,
                cypherMintingRound: market.mintingRound,
                groupSigner: this.group.state.signerKey,
                cAssetMint: token.mint,
                userOwner: this.client.program.provider.wallet.publicKey,
                dexProgram: this.client.dexPID,
                dex: {
                    market: market.dexMarket,
                    openOrders: this.user.getOpenOrdersAddress(
                        this.client,
                        cAssetMint
                    ),
                    reqQ: marketProxy.market.decoded.requestQueue,
                    eventQ: marketProxy.market.decoded.eventQueue,
                    bids: marketProxy.market.bidsAddress,
                    asks: marketProxy.market.asksAddress,
                    cypherCAssetVault: token.vault,
                    cypherUser: this.user.address,
                    coinVault: marketProxy.market.decoded.baseVault,
                    pcVault: marketProxy.market.decoded.quoteVault,
                    tokenProgram: TOKEN_PROGRAM_ID,
                    rent: SYSVAR_RENT_PUBKEY,
                },
            },
        });
    }

    async mintCAssets(
        cAssetMint: PublicKey,
        uiAmount: number,
        uiPrice: number
    ) {
        const tx = new Transaction();
        const tokenIdx = this.group.gettokenIdx(
            this.client,
            cAssetMint
        );
        const userCAsset = this.user.getCAsset(tokenIdx);
        if (userCAsset?.ooInfo && !userCAsset.ooInfo.coinFree.isZero()) {
            const settleFundsInstr = await this.makeSettleFundsInstr(
                cAssetMint
            );
            tx.add(settleFundsInstr);
        } else {
            const initOpenOrdersInstr = await this.makeInitOpenOrdersInstr(
                cAssetMint
            );
            if (initOpenOrdersInstr) {
                tx.add(initOpenOrdersInstr);
            }
        }
        const mintInstr = this.makeMintCAssetsInstr(
            cAssetMint,
            uiAmount,
            uiPrice
        );
        tx.add(mintInstr);
        return this.client.provider.send(tx);
    }

    async burnCAssets(cAssetMint: PublicKey, uiAmount: number) {
        const amount = TokenAmount.toSplCAssetAmount(uiAmount);
        const token = this.group.getCypherToken(this.client, cAssetMint);
        const market = this.group.getCypherMarket(this.client, cAssetMint);
        return this.client.program.rpc.burnCAssets(amount, {
            accounts: {
                cypherGroup: this.group.address,
                cypherMintingRound: market.mintingRound,
                cypherUser: this.user.address,
                groupSigner: this.group.state.signerKey,
                cAssetMint: token.mint,
                cypherCAssetVault: token.vault,
                userOwner: this.client.program.provider.wallet.publicKey,
                tokenProgram: TOKEN_PROGRAM_ID,
            },
        });
    }

    async depositUSDCToMarginAccount(uiAmount: number) {
        const amount = TokenAmount.toSplUSDCAmount(uiAmount);
        return this.client.program.rpc.depositUsdcToMarginAccount(amount, {
            accounts: {
                cypherGroup: this.group.address,
                cypherUser: this.user.address,
                depositFrom: this.associatedUSDCAddress,
                cypherPcVault: this.group.quoteVault,
                depositAuthority: this.client.program.provider.wallet.publicKey,
                tokenProgram: TOKEN_PROGRAM_ID,
            },
        });
    }

    async withdrawUSDCFromMarginAccount(uiAmount: number) {
        const amount = TokenAmount.toSplUSDCAmount(uiAmount);
        return this.client.program.rpc.withdrawUsdcFromMarginAccount(amount, {
            accounts: {
                cypherGroup: this.group.address,
                cypherUser: this.user.address,
                groupSigner: this.group.state.signerKey,
                cypherPcVault: this.group.quoteVault,
                withdrawTo: this.associatedUSDCAddress,
                userOwner: this.client.program.provider.wallet.publicKey,
                tokenProgram: TOKEN_PROGRAM_ID,
            },
        });
    }

    makePlaceOrderInstr(
        cAssetMint: PublicKey,
        params: PlaceOrderParams
    ) {
        const token = this.group.getCypherToken(this.client, cAssetMint);
        const marketProxy = this.group.getMarketProxy(this.client, cAssetMint);
        const payerVault =
            params.side === "buy" ? this.group.quoteVault : token.vault;
        const openOrdersAddressKey = this.user.getOpenOrdersAddress(
            this.client,
            cAssetMint
        );
        return marketProxy.instruction.newOrderV3({
            ...params,
            owner: this.client.program.provider.wallet.publicKey,
            payer: payerVault,
            openOrdersAddressKey,
        });
    }

    async placeOrder(cAssetMint: PublicKey, params: PlaceOrderParams) {
        const tx = new Transaction();
        const initOpenOrdersInstr = await this.makeInitOpenOrdersInstr(
            cAssetMint
        );
        if (initOpenOrdersInstr) {
            tx.add(initOpenOrdersInstr);
        }
        tx.add(this.makePlaceOrderInstr(cAssetMint, params));
        return this.client.provider.send(tx);
    }

    makeSettleFundsInstr(cAssetMint: PublicKey) {
        const token = this.group.getCypherToken(this.client, cAssetMint);
        const marketProxy = this.group.getMarketProxy(this.client, cAssetMint);
        const openOrdersAddr = this.user.getOpenOrdersAddress(
            this.client,
            cAssetMint
        );
        return marketProxy.instruction.settleFunds(
            openOrdersAddr,
            this.client.walletPubkey,
            token.vault,
            this.group.quoteVault,
            null
        );
    }

    async settleFunds(cAssetMint: PublicKey) {
        const tx = new Transaction();
        tx.add(this.makeSettleFundsInstr(cAssetMint));
        return this.client.provider.send(tx);
    }

    async placeOrderAndSettle(cAssetMint: PublicKey, params: PlaceOrderParams) {
        const tx = new Transaction();
        const initOpenOrdersInstr = await this.makeInitOpenOrdersInstr(
            cAssetMint
        );
        tx.add(makeRequestCuInstr(this.client.walletPubkey, 300_000));
        if (initOpenOrdersInstr) {
            tx.add(initOpenOrdersInstr);
        }
        tx.add(this.makePlaceOrderInstr(cAssetMint, params));
        tx.add(this.makeSettleFundsInstr(cAssetMint));
        return this.client.provider.send(tx);
    }

    makeCancelOrderInstr(cAssetMint: PublicKey, order: Order) {
        const marketProxy = this.group.getMarketProxy(this.client, cAssetMint);
        return marketProxy.instruction.cancelOrder(
            this.client.program.provider.wallet.publicKey,
            order
        );
    }

    async cancelOrderAndSettle(cAssetMint: PublicKey, order: Order) {
        const tx = new Transaction();
        tx.add(this.makeCancelOrderInstr(cAssetMint, order));
        tx.add(this.makeSettleFundsInstr(cAssetMint));
        return this.client.provider.send(tx);
    }

    makePrepMarginExecuteInstr(cAssetMint: PublicKey) {
        const token = this.group.getCypherToken(this.client, cAssetMint);
        const market = this.group.getCypherMarket(this.client, cAssetMint);
        const marketProxy = this.group.getMarketProxy(this.client, cAssetMint);
        const openOrdersAddr = this.user.getOpenOrdersAddress(
            this.client,
            cAssetMint
        );
        const [marketAuthority] = PermissionedMarket.marketAuthority(
            market.dexMarket,
            this.client.cypherPID
        );
        const vaultSigner = utils.publicKey.createProgramAddressSync(
            [
                market.dexMarket.toBuffer(),
                marketProxy.market.decoded.vaultSignerNonce.toArrayLike(
                    Buffer,
                    "le",
                    8
                ),
            ],
            this.client.dexPID
        );
        return this.client.program.instruction.prepMarginExecute({
            accounts: {
                cypherGroup: this.group.address,
                cypherMintingRound: market.mintingRound,
                groupSigner: this.group.state.signerKey,
                cypherUser: this.user.address,
                userOwner: this.client.walletPubkey,
                cAssetMint,
                cypherCAssetVault: token.vault,
                cypherPcVault: this.group.quoteVault,
                dex: {
                    market: market.dexMarket,
                    bids: marketProxy.market.bidsAddress,
                    asks: marketProxy.market.asksAddress,
                    openOrders: openOrdersAddr,
                    eventQ: marketProxy.market.decoded.eventQueue,
                    coinVault: marketProxy.market.decoded.baseVault,
                    pcVault: marketProxy.market.decoded.quoteVault,
                    marketAuthority,
                    vaultSigner,
                    dexProgram: this.client.dexPID,
                    tokenProgram: TOKEN_PROGRAM_ID,
                },
            },
        });
    }

    makeMarginExecuteInstr(cAssetMint: PublicKey) {
        const market = this.group.getCypherMarket(this.client, cAssetMint);
        return this.client.program.instruction.executeMargin(cAssetMint, {
            accounts: {
                cypherGroup: this.group.address,
                cypherMintingRound: market.mintingRound,
                groupSigner: this.group.state.signerKey,
                cypherUser: this.user.address,
                userOwner: this.client.walletPubkey,
                userUsdcWallet: this.associatedUSDCAddress,
                cypherPcVault: this.group.quoteVault,
                tokenProgram: TOKEN_PROGRAM_ID,
            },
        });
    }

    async executePosition(cAssetMint: PublicKey) {
        const tokenIdx = this.group.gettokenIdx(
            this.client,
            cAssetMint
        );
        const userCAsset = this.user.getCAsset(tokenIdx);
        if (!userCAsset) {
            throw new Error("You don't have position for this market.");
        }
        const tx = new Transaction();
        tx.add(makeRequestCuInstr(this.client.walletPubkey, 300_000));
        if (await this.hasOpenOrders(cAssetMint)) {
            tx.add(this.makePrepMarginExecuteInstr(cAssetMint));
        }
        tx.add(this.makeMarginExecuteInstr(cAssetMint));
        return this.client.provider.send(tx);
    }
}
