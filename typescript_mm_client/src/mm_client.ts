
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
    DEFAULT_GROUP_ARGS,
    Cluster,
    CypherClient,
    CypherGroup,
    CypherUserController,
    makeNewOrderV3Ix,
    makeSettleFundsIx,
    makeCancelOrderV2Ix,
    makeDepositCollateralIx,
    CypherUser,
    Side,

} from "@chugach-foundation/cypher-client";
import {
    uiToSplAmount,
    uiToSplPrice
} from "@chugach-foundation/cypher-client/lib/utils/tokenAmount";


const DEX_TAKER_FEE = 0.000025;

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
    trader: CypherUser
    traderctr: CypherUserController
    traderk: Keypair


    private cAssetMint: PublicKey
    connection: Connection
    private baseLotSize: BN
    private quoteLotSize: BN
    private quoteDecimals: number
    private baseDecimals: number

    private constructor(cInfo: cAssetMarketInfo, lmarket: LiveMarket, connection: Connection, baseDecimals: number, traderctr: CypherUserController, traderk: Keypair) {
        this.cAssetMint = cInfo.cAssetMint;
        this.lmarket = lmarket;
        this.connection = connection;
        this.traderk = traderk;
        this.trader = traderctr.user;
        this.traderctr = traderctr;
        this.baseLotSize = lmarket.market.decoded.baseLotSize;
        this.quoteLotSize = lmarket.market.decoded.quoteLotSize;
        this.quoteDecimals = DEFAULT_GROUP_ARGS.quoteTokenDecimals;
        this.baseDecimals = baseDecimals;
    }

    static async load(cAssetMint: PublicKey, cluster: Cluster, rpc: string, group: CypherGroup, traderCtr: CypherUserController, traderk: Keypair): Promise<CypherMMClient> { // , 
        const connection = new Connection(rpc, "processed")
        const dexkey = group.getDexMarket(cAssetMint).address;

        const cInfo: cAssetMarketInfo = {
            cAssetMarketProgramAddress: CONFIGS[cluster].DEX_PID,
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

        return new CypherMMClient(cInfo, lmarket, connection, baseDecimals, traderCtr, traderk); //, 

    }

    async depositMarginCollateralIx(amount: BN): Promise<TransactionInstruction> {
        // @ts-ignore
        return await makeDepositCollateralIx(this.trader, amount)
    }

    async placeOrderIx(price: number, size: number, side: Side): Promise<TransactionInstruction> {
        let pricebn = uiToSplPrice(price, this.baseDecimals, this.quoteDecimals);
        let amountbn = uiToSplAmount(size, this.baseDecimals);
        pricebn = pricebn.mul(this.lmarket.market.decoded.baseLotSize).div(this.lmarket.market.decoded.quoteLotSize);
        amountbn = amountbn.div(this.lmarket.market.decoded.baseLotSize);

        return await makeNewOrderV3Ix(
            this.trader,
            this.cAssetMint,
            side,
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
        )
    }

    async cancelOrderIx(orders: Order[]): Promise<TransactionInstruction[]> {
        let ixs = []
        //Fix this inefficient bs -- keep track of orders with ws? -- TODO
        for (let i = 0; i < orders.length; i++) {
            let ix = await makeCancelOrderV2Ix(this.trader, this.cAssetMint, orders[i]);
            ixs.push(ix);
            // console.log(ix)
        }
        // console.log(ixs);
        return ixs;
    }

    async settleFundsIx(): Promise<TransactionInstruction[]> {
        let ixs = []
        ixs.push(await makeSettleFundsIx(this.trader, this.cAssetMint));
        return ixs
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

    getTopSpread() {
        return this.lmarket.getTopSpread();
    }

    async getPositionLong(): Promise<BN> {
        return this.traderctr.user.getTokenViewer(this.cAssetMint).deposits;
    }





    // async makeSettleFundsIxuction() {
    //     let ixs = []
    //     ixs.push(await this.traderctr.makeSettleFundsIx(this.cAssetMint));
    //     return ixs
    // }




    // async makeCancelBidOrdersInstructions(orders: Order[]): Promise<TransactionInstruction[]> {

    //     let ixs = []
    //     //Fix this inefficient bs -- keep track of orders with ws? -- TODO
    //     for (let i = 0; i < orders.length; i++) {
    //         let ix = await this.traderctr.makeCancelOrderV2Ix(this.cAssetMint, orders[i]);
    //         ixs.push(ix);
    //         console.log(ix)
    //     }
    //     /*toCancel.map(
    //         async (order) => {
    //             let ix = await this.traderctr.makeCancelOrderV2Ix(this.cAssetMint, order);
    //             console.log(ix);
    //             ixs.push(
    //                 ix
    //             )
    //         });
    //         */
    //     console.log(ixs);
    //     return ixs;
    // }


    // async makeBidInstruction(price: number, size: number): Promise<TransactionInstruction> {
    //     let pricebn = uiToSplPrice(price, this.baseDecimals, this.quoteDecimals);
    //     let amountbn = uiToSplAmount(size, this.baseDecimals);
    //     pricebn = pricebn.mul(this.lmarket.market.decoded.baseLotSize).div(this.lmarket.market.decoded.quoteLotSize);
    //     amountbn = amountbn.div(this.lmarket.market.decoded.baseLotSize);

    //     return await makeNewOrderV3Ix(
    //         this.trader,
    //         this.cAssetMint,
    //         "buy",
    //         pricebn,
    //         amountbn,
    //         // @ts-ignore
    //         new BN(
    //             this.lmarket.market.decoded.quoteLotSize.toNumber() * (1 + DEX_TAKER_FEE) * 10000
    //         )
    //             .mul(amountbn.mul(pricebn))
    //             .div(new BN(10000)),
    //         "postOnly",
    //         "decrementTake"
    //     );
    // }

    // async makeAskInstruction(price: number, size: number): Promise<TransactionInstruction> {
    //     let pricebn = uiToSplPrice(price, this.baseDecimals, this.quoteDecimals);
    //     let amountbn = uiToSplAmount(size, this.baseDecimals);
    //     pricebn = pricebn.mul(this.lmarket.market.decoded.baseLotSize).div(this.lmarket.market.decoded.quoteLotSize);
    //     amountbn = amountbn.div(this.lmarket.market.decoded.baseLotSize);
    //     return await this.traderctr.makeNewOrderV3Ix(
    //         this.cAssetMint,
    //         "sell",
    //         pricebn,
    //         amountbn,
    //         // @ts-ignore
    //         new BN(
    //             this.lmarket.market.decoded.quoteLotSize.toNumber() * (1 + DEX_TAKER_FEE) * 10000
    //         )
    //             .mul(amountbn.mul(pricebn))
    //             .div(new BN(10000)),
    //         "postOnly",
    //         "decrementTake"
    //     );
    // }

    // async depositMarketBidCollateralInstr(cAssetMint: PublicKey, amount: BN): Promise<TransactionInstruction> {
    //     // console.log(
    //     //     'Deposit market collateral for CAsset: ',
    //     //     cAssetMint.toString()
    //     // );
    //     // console.log('Amount: ', amount.toNumber());
    //     this.traderctr.group.validateMarket(cAssetMint);
    //     return await this.traderctr.client.methods
    //         .depositCollateral(amount)
    //         .accounts({
    //             cypherGroup: this.traderctr.group.address,
    //             cypherUser: this.traderctr.user.address,
    //             cypherPcVault: this.traderctr.group.quoteVault,
    //             depositFrom: await this.traderctr.client.getAssociatedUSDCAddress(),
    //             userSigner: this.traderctr.client.walletPubkey,
    //             tokenProgram: TOKEN_PROGRAM_ID
    //         })
    //         .instruction();
    // }
}