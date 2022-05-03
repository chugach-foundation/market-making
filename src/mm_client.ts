
import {
    PublicKey,
    Connection,
    Keypair,
    TransactionInstruction
} from "@solana/web3.js";
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

const quotedecimals = 4;
const basedecimals = 4;
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

    private constructor(cInfo: cAssetMarketInfo, lmarket: LiveMarket, connection: Connection, bidctr: CypherUserController, mintctr?: CypherUserController) {
        this.cAssetMint = cInfo.cAssetMint;
        this.lmarket = lmarket;
        this.connection = connection;
        this.bidctr = bidctr;
        this.mintctr = mintctr ?? bidctr;
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

        if (minterKeyPath) {
            const mintk = loadPayer(minterKeyPath);
            const mintctr = await CypherUserController.loadOrCreate(new CypherClient(cluster, new NodeWallet(mintk), { commitment: "processed", skipPreflight: true }), groupAddr);
            return new CypherMMClient(cInfo, lmarket, connection, bidctr.userController, mintctr.userController);
        }
        else {
            return new CypherMMClient(cInfo, lmarket, connection, bidctr.userController);
        }

    }

    get mintPayer(): Keypair {
        return (this.mintctr.client.anchorProvider.wallet as NodeWallet).payer;
    }

    get bidPayer(): Keypair {
        return (this.bidctr.client.anchorProvider.wallet as NodeWallet).payer;
    }

    async makeBidInstruction(price: number, size: number): Promise<TransactionInstruction> {
        return await this.bidctr.makeNewOrderV3Instr(
            this.cAssetMint,
            "buy",
            uiToSplPrice(price, basedecimals, quotedecimals),
            uiToSplAmount(size, basedecimals),
            u64_max,
            "postOnly",
            "decrementTake"
        );
    }

    async makeAskInstruction(price: number, size: number): Promise<TransactionInstruction> {
        return await this.bidctr.makeNewOrderV3Instr(
            this.cAssetMint,
            "sell",
            uiToSplPrice(price, basedecimals, quotedecimals),
            uiToSplAmount(size, basedecimals),
            u64_max,
            "postOnly",
            "decrementTake"
        );
    }

    /// adjsuted for new client lib
    async makeMintInstruction(price: number, size: number): Promise<TransactionInstruction> {
        return await this.mintctr.makeMintAndSellInstr(this.cAssetMint, uiToSplPrice(price, basedecimals, quotedecimals), uiToSplAmount(size, basedecimals));
    }

    getTopSpread() {
        return this.lmarket.getTopSpread();
    }

    makeSettleFundsInstruction() {
        return this.bidctr.makeSettleFundsInstr(this.cAssetMint);
    }

    async makeCancelAllOrdersInstructions(orders: Order[]): Promise<TransactionInstruction[]> {

        let ixs = []
        //Fix this inefficient bs -- keep track of orders with ws? -- TODO
        const toCancel = orders;
        toCancel.map(
            (order) => {
                ixs.push(
                    this.bidctr.makeCancelOrderV2Instr(this.cAssetMint, order)
                )
            });
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