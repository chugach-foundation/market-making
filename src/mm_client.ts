
import {
    PublicKey,
    Connection,
    Keypair,
    TransactionInstruction
} from "@solana/web3.js";
import { TOKEN_PROGRAM_ID } from '@solana/spl-token';
import { Market } from "@project-serum/serum";
import { LiveMarket } from "./livemarket/live_market";
import { loadPayer } from "./utils";
import NodeWallet from "@project-serum/anchor/dist/cjs/nodewallet";
import { Order } from "@project-serum/serum/lib/market";
import { BN } from "@project-serum/anchor";
import {
    CONFIGS,
    Cluster,
    CypherClient,
    CypherGroup,
    CypherUserController
} from "@chugach-foundation/cypher-client";
import {
    uiToSplAmount,
    uiToSplPrice
} from "@chugach-foundation/cypher-client/lib/utils/tokenAmount";


const u64_max = new BN("18446744073709551615");
const DEX_TAKER_FEE = 0.0004;

export type cAssetMarketInfo = {
    cAssetMarketProgramAddress: PublicKey,
    cAssetOrderbookAddress: PublicKey,
    cAssetMint: PublicKey,
}

export type OutOrdersInfo = {
    bidSize?: number,
    bidPrice?: number,
    askSize?: number,
    askPrice?: number
    orders?: Order[]
}

export class CypherMMClient {

    private lmarket: LiveMarket
    bidctr: CypherUserController
    mintctr: CypherUserController
    private cAssetMint: PublicKey
    connection: Connection
    private baseLotSize: BN
    private quoteLotSize: BN
    private quoteDecimals: number
    private baseDecimals: number

    private constructor(cInfo: cAssetMarketInfo, lmarket: LiveMarket, connection: Connection, baseDecimals: number, bidctr: CypherUserController, mintctr?: CypherUserController) {
        this.cAssetMint = cInfo.cAssetMint;
        this.lmarket = lmarket;
        this.connection = connection;
        this.bidctr = bidctr;
        this.mintctr = mintctr ?? bidctr;
        this.baseLotSize = lmarket.market.decoded.baseLotSize;
        this.quoteLotSize = lmarket.market.decoded.quoteLotSize;
        //HARDCODED RANDOM DECIMAL!!
        this.quoteDecimals = 6;
        this.baseDecimals = baseDecimals;
    }

    static async load(cAssetMint: PublicKey, cluster: Cluster, rpc: string, groupAddr: PublicKey, bidderKeyPath: string, minterKeyPath?: string): Promise<CypherMMClient> {
        const connection = new Connection(rpc, "processed")




        const bidk = loadPayer(bidderKeyPath);
        const bidclient = new CypherClient(cluster, new NodeWallet(bidk), { commitment: "processed", skipPreflight: true });
        const bidctr = await CypherUserController.loadOrCreate(bidclient, groupAddr);
        bidctr.userController
        const group = await CypherGroup.load(bidclient, groupAddr);
        const dexkey = group.getDexMarket(cAssetMint).address;

        const cInfo: cAssetMarketInfo = {
            cAssetMarketProgramAddress: CONFIGS.devnet.DEX_PID,
            cAssetOrderbookAddress: dexkey,
            cAssetMint: cAssetMint
        }
        const lmarket = new LiveMarket(
            connection,
            await Market.load(connection,
                cInfo.cAssetOrderbookAddress,
                {
                    commitment: "processed",
                    skipPreflight: true
                },
                cInfo.cAssetMarketProgramAddress)
        );

        await lmarket.start((info) => { });
        const baseDecimals = group.getTokenViewer(cAssetMint).decimals;
        if (minterKeyPath) {
            const mintk = loadPayer(minterKeyPath);
            const mintctr = await CypherUserController.loadOrCreate(new CypherClient(cluster, new NodeWallet(mintk), { commitment: "processed", skipPreflight: true }), groupAddr);
            return new CypherMMClient(cInfo, lmarket, connection, baseDecimals, bidctr.userController, mintctr.userController);
        }
        else {
            return new CypherMMClient(cInfo, lmarket, connection, baseDecimals, bidctr.userController);
        }

    }

    get mintPayer(): Keypair {
        return (this.mintctr.client.anchorProvider.wallet as NodeWallet).payer;
    }

    get bidPayer(): Keypair {
        return (this.bidctr.client.anchorProvider.wallet as NodeWallet).payer;
    }


    async makeBidInstruction(price: number, size: number): Promise<TransactionInstruction> {
        let pricebn = uiToSplPrice(price, this.baseDecimals, this.quoteDecimals);
        let amountbn = uiToSplAmount(size, this.baseDecimals);
        pricebn = pricebn.mul(this.lmarket.market.decoded.baseLotSize).div(this.lmarket.market.decoded.quoteLotSize);
        amountbn = amountbn.div(this.lmarket.market.decoded.baseLotSize);

        return await this.bidctr.makeNewOrderV3Instr(
            this.cAssetMint,
            "buy",
            pricebn,
            amountbn,
            // @ts-ignore
            new BN(
                this.lmarket.market.decoded.quoteLotSize.toNumber() * (1 + DEX_TAKER_FEE) * 10000
            )
                .mul(amountbn.mul(pricebn))
                .div(new BN(10000)),
            "postOnly",
            "decrementTake"
        );
    }

    async makeAskInstruction(price: number, size: number): Promise<TransactionInstruction> {
        let pricebn = uiToSplPrice(price, this.baseDecimals, this.quoteDecimals);
        let amountbn = uiToSplAmount(size, this.baseDecimals);
        pricebn = pricebn.mul(this.lmarket.market.decoded.baseLotSize).div(this.lmarket.market.decoded.quoteLotSize);
        amountbn = amountbn.div(this.lmarket.market.decoded.baseLotSize);
        return await this.bidctr.makeNewOrderV3Instr(
            this.cAssetMint,
            "sell",
            pricebn,
            amountbn,
            // @ts-ignore
            new BN(
                this.lmarket.market.decoded.quoteLotSize.toNumber() * (1 + DEX_TAKER_FEE) * 10000
            )
                .mul(amountbn.mul(pricebn))
                .div(new BN(10000)),
            "postOnly",
            "decrementTake"
        );
    }

    async depositMarketMintCollateralInstr(cAssetMint: PublicKey, amount: BN) {
        // console.log(
        //     'Deposit market collateral for CAsset: ',
        //     cAssetMint.toString()
        // );
        // console.log('Amount: ', amount.toNumber());
        this.mintctr.group.validateMarket(cAssetMint);
        return await this.mintctr.client.methods
            // @ts-ignore
            .depositMarketCollateral(cAssetMint, amount)
            .accounts({
                cypherGroup: this.mintctr.group.address,
                cypherUser: this.mintctr.user.address,
                cypherPcVault: this.mintctr.group.quoteVault,
                depositFrom: await this.mintctr.client.getAssociatedUSDCAddress(),
                userSigner: this.mintctr.client.walletPubkey,
                tokenProgram: TOKEN_PROGRAM_ID
            })
            .instruction();
    }

    async depositMarketBidCollateralInstr(cAssetMint: PublicKey, amount: BN) : Promise<TransactionInstruction> {
        // console.log(
        //     'Deposit market collateral for CAsset: ',
        //     cAssetMint.toString()
        // );
        // console.log('Amount: ', amount.toNumber());
        this.bidctr.group.validateMarket(cAssetMint);
        return await this.bidctr.client.methods
        .depositCollateral(amount)
        .accounts({
        cypherGroup: this.bidctr.group.address,
        cypherUser: this.bidctr.user.address,
        cypherPcVault: this.bidctr.group.quoteVault,
        depositFrom: await this.bidctr.client.getAssociatedUSDCAddress(),
        userSigner: this.bidctr.client.walletPubkey,
        tokenProgram: TOKEN_PROGRAM_ID
    })
        .instruction();
    }

    async depositMintCollateralInstruction(price: number, size: number): Promise<TransactionInstruction> {
        // fix to pull cratio for given markert from onchain program
        return await this.depositMarketMintCollateralInstr(this.cAssetMint, uiToSplAmount(price * size * 1.75, this.quoteDecimals))
    }

    async makeMintInstruction(price: number, size: number): Promise<TransactionInstruction> {
        return await this.mintctr.makeMintAndSellInstr(this.cAssetMint, uiToSplPrice(price, this.baseDecimals, this.quoteDecimals), uiToSplAmount(size, this.baseDecimals));
    }

    getTopSpread() {
        return this.lmarket.getTopSpread();
    }

    async withdrawCollateralInstr(amount: BN) {
        // console.log('Withdraw collateral, amount: ', amount.toNumber());
        return await this.mintctr.client.methods
            //@ts-ignore
            .withdrawCollateral(amount)
            .accounts({
                cypherGroup: this.mintctr.group.address,
                vaultSigner: this.mintctr.group.vaultSigner,
                cypherUser: this.mintctr.user.address,
                cypherPcVault: this.mintctr.group.quoteVault,
                withdrawTo: await this.mintctr.client.getAssociatedUSDCAddress(),
                userSigner: this.mintctr.client.walletPubkey,
                tokenProgram: TOKEN_PROGRAM_ID
            })
            .instruction();
    }

    async makeSettleFundsInstruction() {
        let ixs = []
        ixs.push(await this.bidctr.makeSettleFundsInstr(this.cAssetMint));
        ixs.push(await this.mintctr.makeSettleFundsInstr(this.cAssetMint));
        // add instruction to withdraw usdc from minter margin account in order to increase capital efficiency
        return ixs
    }

    async makeCancelBidOrdersInstructions(orders: Order[]): Promise<TransactionInstruction[]> {

        let ixs = []
        //Fix this inefficient bs -- keep track of orders with ws? -- TODO
        for(let i = 0; i < orders.length; i++){
            let ix = await this.bidctr.makeCancelOrderV2Instr(this.cAssetMint, orders[i]);
            ixs.push(ix);
            console.log(ix)
        }
        /*toCancel.map(
            async (order) => {
                let ix = await this.bidctr.makeCancelOrderV2Instr(this.cAssetMint, order);
                console.log(ix);
                ixs.push(
                    ix
                )
            });
            */
        console.log(ixs);
        return ixs;
    }

    async withdrawMarketCollateralInstr(cAssetMint: PublicKey, amount: BN) {
        // console.log(
        //   'Withdraw market collateral for CAsset: ',
        //   cAssetMint.toString()
        // );
        // console.log('Amount: ', amount.toNumber());
        this.mintctr.group.validateMarket(cAssetMint);
        return await this.mintctr.client.methods
            //@ts-ignore
            .withdrawMarketCollateral(cAssetMint, amount)
            .accounts({
                cypherGroup: this.mintctr.group.address,
                vaultSigner: this.mintctr.group.vaultSigner,
                cypherUser: this.mintctr.user.address,
                cypherPcVault: this.mintctr.group.quoteVault,
                withdrawTo: await this.mintctr.client.getAssociatedUSDCAddress(),
                userSigner: this.mintctr.client.walletPubkey,
                tokenProgram: TOKEN_PROGRAM_ID
            })
            .instruction();
    }

    async makeCancelMintOrdersAndWithdrawMktCollateralInstructions(orders: Order[]): Promise<TransactionInstruction[]> {

        let ixs = []
        //Fix this inefficient bs -- keep track of orders with ws? -- TODO
        for(let i = 0; i < orders.length; i++){
            ixs.push(await this.mintctr.makeCancelOrderV2Instr(this.cAssetMint, orders[i]));
        }
        return ixs;

    }

    async getOutOrdersInfo(ctr: CypherUserController): Promise<OutOrdersInfo> {
        const orders = await ctr.user.getMarketOrders(this.cAssetMint);
        let bidsize = 0, bidprice = 0, asksize = 0, askprice = 0;
        orders.map(
            (order) => {
                if (order.side == "buy") {
                    bidsize += order.size;
                    bidprice = order.price
                }
                else {
                    asksize += order.size;
                    askprice = order.price;
                }
            }
        )
        return {
            orders: orders,
            bidPrice: bidprice,
            bidSize: bidsize,
            askPrice: askprice,
            askSize: asksize
        };


    }

    async getPositionLong(): Promise<BN> {
        return this.bidctr.user.getTokenViewer(this.cAssetMint).deposits;
    }

    async getPositionMinted(): Promise<BN> {
        return this.mintctr.user.getMarketViewer(this.cAssetMint).debtSharesCirculating;
    }

}