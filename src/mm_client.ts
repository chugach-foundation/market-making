
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
    private ctr : CypherUserController
    private cAssetMint : PublicKey
    connection : Connection
    payer : Keypair

    private constructor(cInfo : cAssetMarketInfo, ctr : CypherUserController, lmarket : LiveMarket, connection: Connection, payer : Keypair){
        this.cAssetMint = cInfo.cAssetMint;
        this.lmarket = lmarket;
        this.ctr = ctr;
        this.connection = connection;
        this.payer = payer;
    }

    static async load(cInfo : cAssetMarketInfo, cluster : CLUSTER, payerPath : string, rpc : string, groupAddr : PublicKey) : Promise<CypherMMClient>{
        const payer = loadPayer(payerPath);
        const ctr = await CypherUserController.load(new CypherClient(cluster, new NodeWallet(payer), {commitment : "processed", skipPreflight : true}), groupAddr, "ACCOUNT");
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
        return new CypherMMClient(cInfo, ctr, lmarket, connection, payer);
    }

    makeBidInstruction(price : number, size : number) : TransactionInstruction{
        return this.ctr.makePlaceOrderInstr(this.cAssetMint, 
            {
                side : "buy",
                orderType : "postOnly",
                price : price,
                size : size,
                selfTradeBehavior : "decrementTake"
            });
    }

    makeAskInstruction(price : number, size : number) : TransactionInstruction{
        return this.ctr.makePlaceOrderInstr(this.cAssetMint, 
            {
                side : "sell",
                orderType : "postOnly",
                price : price,
                size : size,
                selfTradeBehavior : "decrementTake"
            });
    }

    makeMintInstruction(price : number, size : number) : TransactionInstruction{
        return this.ctr.makeMintCAssetsInstr(this.cAssetMint, size, price);
    }

    getTopSpread(){
        return this.lmarket.getTopSpread();
    }

    makeSettleFundsInstruction(){
        return this.ctr.makeSettleFundsInstr(this.cAssetMint);
    }

    async makeCancelAllOrdersInstructions(orders : Order[]) : Promise<TransactionInstruction[]>{
        
        let ixs = []
        //Fix this inefficient bs -- keep track of orders with ws? -- TODO
        const toCancel = orders;
        toCancel.map(
            (order) =>
        {
            ixs.push(
                this.ctr.makeCancelOrderInstr(this.cAssetMint, order)
            )
        });
        return ixs;
    }

    async getOutOrdersInfo() : Promise<OutOrdersInfo>{
        const orders = await this.ctr.user.getMarketOrders(this.ctr.client, this.cAssetMint);
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
    async getPosition(){

    }

}