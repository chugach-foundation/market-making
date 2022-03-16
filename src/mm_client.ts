
import { CypherClient } from "@chugach-foundation/cypher-client";
import {PublicKey, Connection} from "@solana/web3.js";
import {Market} from "@project-serum/serum";
import { LiveMarket } from "./livemarket/live_market";
import {CypherUserController} from "./mmuser";
import { loadPayer } from "./utils";
import { CLUSTER } from "@chugach-foundation/cypher-client";
import {Keypair, TransactionInstruction} from "@solana/web3.js"
import NodeWallet from "@project-serum/anchor/dist/cjs/nodewallet";
import { Order } from "@project-serum/serum/lib/market";

export type cAssetMarketInfo = {
    cAssetMarketProgramAddress? : PublicKey,
    cAssetOrderbookAddress? : PublicKey,
    cAssetMint? : PublicKey,
}

export type OutOrdersInfo = {
    bidSize? : number,
    bidPrice? : number,
    askSize? : number,
    askPrice? : number
    orders? : Order[]
}

export class CypherMMClient{

    private lmarket : LiveMarket
    bidctr : CypherUserController
    mintctr : CypherUserController
    private cAssetMint : PublicKey
    connection : Connection

    private constructor(cInfo : cAssetMarketInfo, lmarket : LiveMarket, connection: Connection, bidctr : CypherUserController, mintctr? : CypherUserController){
        this.cAssetMint = cInfo.cAssetMint;
        this.lmarket = lmarket;
        this.connection = connection;
        this.bidctr = bidctr;
        this.mintctr = mintctr ?? bidctr;
    }

    static async load(cInfo : cAssetMarketInfo, cluster : CLUSTER, rpc : string, groupAddr : PublicKey, bidderKeyPath : string, minterKeyPath? : string) : Promise<CypherMMClient>{
        const connection = new Connection(rpc, "processed")
        const lmarket = new LiveMarket(
            connection, 
            await Market.load(connection, 
            cInfo.cAssetOrderbookAddress,
            {
                commitment : "processed",
                skipPreflight : true
            },
            cInfo.cAssetMarketProgramAddress)
            );
        await lmarket.start((info) => {});
        
        
        const bidk = loadPayer(bidderKeyPath);
        const bidctr = await CypherUserController.load(new CypherClient(cluster, new NodeWallet(bidk), {commitment : "processed", skipPreflight : true}), groupAddr, "ACCOUNT");
        if(minterKeyPath){
            const mintk = loadPayer(minterKeyPath);
            const mintctr = await CypherUserController.load(new CypherClient(cluster, new NodeWallet(mintk), {commitment : "processed", skipPreflight : true}), groupAddr, "ACCOUNT");
            return new CypherMMClient(cInfo, lmarket, connection, bidctr, mintctr);
        }
        else{
            return new CypherMMClient(cInfo, lmarket, connection, bidctr);
        }
        
    }

    get mintPayer() : Keypair{
        return (this.mintctr.client.provider.wallet as NodeWallet).payer;
    }

    get bidPayer() : Keypair{
        return (this.bidctr.client.provider.wallet as NodeWallet).payer;
    }

    makeBidInstruction(price : number, size : number) : TransactionInstruction{
        return this.bidctr.makePlaceOrderInstr(this.cAssetMint, 
            {
                side : "buy",
                orderType : "postOnly",
                price : price,
                size : size,
                selfTradeBehavior : "decrementTake"
            });
    }

    makeAskInstruction(price : number, size : number) : TransactionInstruction{
        return this.bidctr.makePlaceOrderInstr(this.cAssetMint, 
            {
                side : "sell",
                orderType : "postOnly",
                price : price,
                size : size,
                selfTradeBehavior : "decrementTake"
            });
    }

    makeMintInstruction(price : number, size : number) : TransactionInstruction{
        return this.mintctr.makeMintCAssetsInstr(this.cAssetMint, size, price);
    }

    getTopSpread(){
        return this.lmarket.getTopSpread();
    }

    makeSettleFundsInstruction(){
        return this.bidctr.makeSettleFundsInstr(this.cAssetMint);
    }

    async makeCancelAllOrdersInstructions(orders : Order[]) : Promise<TransactionInstruction[]>{
        
        let ixs = []
        //Fix this inefficient bs -- keep track of orders with ws? -- TODO
        const toCancel = orders;
        toCancel.map(
            (order) =>
        {
            ixs.push(
                this.bidctr.makeCancelOrderInstr(this.cAssetMint, order)
            )
        });
        return ixs;
    }

    async getOutOrdersInfo(ctr : CypherUserController) : Promise<OutOrdersInfo>{
        const orders = await ctr.user.getMarketOrders(ctr.client, this.cAssetMint);
        let bidsize = 0, bidprice = 0, asksize = 0, askprice = 0;
        orders.map(
            (order) =>
            {
                if(order.side == "buy"){
                    bidsize += order.size;
                    bidprice = order.price
                }
                else{
                    asksize += order.size;
                    askprice = order.price;
                }
            }
        )
        return {
            orders : orders,
            bidPrice : bidprice,
            bidSize : bidsize,
            askPrice : askprice,
            askSize : asksize
        };
        
        
    }

    async getPositionLong(){
        return this.bidctr.user.getTokenViewer(this.bidctr.client, this.cAssetMint).deposits;
    }

    async getPositionMinted(){
        return this.mintctr.user.getMarketViewer(this.mintctr.client, this.cAssetMint).debtSharesCirculating;
    }

}