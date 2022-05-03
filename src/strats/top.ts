import { Cypher } from "@chugach-foundation/cypher-client/lib/generated/types/cypher";
import { CypherMMClient } from "src/mm_client";
import {MM_Strat} from "./mmstrat"
import { MM_Hyperparams } from "./stypes";
import {Transaction, TransactionInstruction} from "@solana/web3.js"
import { wait, FastTXNBuilder} from "../utils";
import { connection } from "@project-serum/common";
import { loadPayer } from "src/utils";
import { Order } from "@project-serum/serum/lib/market";
export class TopOfBookStrat implements MM_Strat{
    mclient: CypherMMClient;
    hparams: MM_Hyperparams;

    constructor(client: CypherMMClient, hparams: MM_Hyperparams){
        this.mclient = client;
        this.hparams = hparams;
    }

    private async run() : Promise<void>{
        while(true){
            try{
            console.log(await this.quoteTop());
            await wait(this.hparams.time_requote);
            }
            catch(e){
                console.error(e);
            }
        }
    }

    async start(): Promise<void> {
        this.run();
        console.log("Initialized Strategy: Top Of Book");
    }

    private async quoteTop(){
        const size = this.hparams.max_size;
        let tbid, task;
        try{
            [tbid, task] = this.mclient.getTopSpread();
        }
        catch(e){
            [tbid, task] = [1, 1000000];
        }
        
        let [qbid, qask] = [tbid + .01, task - .01];
        if(qbid >= qask){
            qbid -=.01;
            qask +=.01
        }
        const ixs = []
        const singers = []
        let bidix : TransactionInstruction;
        let mintix : TransactionInstruction;
        //const mintix = this.mclient.makeAskInstruction(qask, size);

        //TODO -- Modify this segment to avoid checking if buy and if sell now that we've separated into two accounts
        let oinfo = await this.mclient.getOutOrdersInfo(this.mclient.bidctr);
        const bidsize = size, asksize = size;
        let toCancel : Order[] = []

        if(oinfo.bidPrice == qbid || oinfo.bidPrice == qbid-.01){
            //Don't frontrun self, but check if we need to reinforce the order
            //Add the fraction missing cuz queue priority
            qbid = oinfo.bidPrice;
            const dif = bidsize - oinfo.bidSize;
            if(dif > .1){
                bidix = await this.mclient.makeBidInstruction(qbid, dif);
                //singers.push(this.mclient.bidPayer);
            }  
        }
        else{
            bidix = await this.mclient.makeBidInstruction(qbid, bidsize);
            //singers.push(this.mclient.bidPayer);
            oinfo.orders.map(
                (order) => 
                {
                    order.side == "buy" ? toCancel.push(order) : {};
                }
            )
        }
        oinfo = await this.mclient.getOutOrdersInfo(this.mclient.mintctr);
        if(oinfo.askPrice == qask || oinfo.askPrice == qask + .01){
            //Don't frontrun self, but check if we need to reinforce the order
            //Add the fraction missing cuz queue priority
            qask = oinfo.askPrice;
            const dif = asksize - oinfo.askSize;
            if(dif > .1){
                mintix = await this.mclient.makeMintInstruction(qask, dif);
            }
        }
        else{
            mintix = await this.mclient.makeMintInstruction(qask, asksize);
            oinfo.orders.map(
                (order) =>
                {
                    order.side == "sell" ? toCancel.push(order) : {};
                }
            )
        }
        singers.push(this.mclient.mintPayer);
        singers.push(this.mclient.bidPayer);
        const builder = new FastTXNBuilder(this.mclient.mintPayer, this.mclient.connection, singers);
        
        if(toCancel.length) builder.add(await this.mclient.makeCancelAllOrdersInstructions(toCancel));
        if(bidix) builder.add(bidix);
        if(mintix) builder.add(mintix);
        const six = await this.mclient.makeSettleFundsInstruction();
        builder.add(six);
        
        if(builder.ixs.length <= 1){
            return "SKIPPING TXN: NO UPDATES REQUIRED";
        } 
        const {execute} = await builder.build();
        const txh = await execute();
        await this.mclient.connection.confirmTransaction(txh, "processed");
        return txh;
    }
    
}