import { Cypher } from "@chugach-foundation/cypher-client/lib/generated/types/cypher";
import { CypherMMClient } from "src/mm_client";
import {MM_Strat} from "./mmstrat"
import { MM_Hyperparams } from "./stypes";
import {Transaction, TransactionInstruction} from "@solana/web3.js"
import { wait } from "../utils";
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
        const ixs2 = []
        let bidix : TransactionInstruction;
        let mintix : TransactionInstruction;
        //const mintix = this.mclient.makeAskInstruction(qask, size);
        const oinfo = await this.mclient.getOutOrdersInfo();
        const bidsize = size, asksize = size;
        let toCancel : Order[] = []

        if(oinfo.bidPrice == qbid || oinfo.bidPrice == qbid-.01){
            //Don't frontrun self, but check if we need to reinforce the order
            //Add the fraction missing cuz queue priority
            qbid = oinfo.bidPrice;
            const dif = bidsize - oinfo.bidSize;
            if(dif > .1){
                bidix = this.mclient.makeBidInstruction(qbid, dif);
            }  
        }
        else{
            bidix = this.mclient.makeBidInstruction(qbid, bidsize);
            oinfo.orders.map(
                (order) => 
                {
                    order.side == "buy" ? toCancel.push(order) : {};
                }
            )
        }

        if(oinfo.askPrice == qask || oinfo.askPrice == qask + .01){
            //Don't frontrun self, but check if we need to reinforce the order
            //Add the fraction missing cuz queue priority
            qask = oinfo.askPrice;
            const dif = asksize - oinfo.askSize;
            if(dif > .1){
                mintix = this.mclient.makeMintInstruction(qask, dif);
            }
        }
        else{
            mintix = this.mclient.makeMintInstruction(qask, asksize);
            oinfo.orders.map(
                (order) =>
                {
                    order.side == "sell" ? toCancel.push(order) : {};
                }
            )
        }
        //TODO - Separate this logic into a TXNBuilder class

        if(bidix) ixs.push(bidix);
        if(mintix) ixs.push(mintix);

        (await this.mclient.makeCancelAllOrdersInstructions(toCancel)).map(
            (ix) =>
        {
            ixs2.push(ix)
        });
        const six = this.mclient.makeSettleFundsInstruction();
        const txn = new Transaction();
        ixs2.map((ix)=>
        {
            txn.add(ix);
        });
        txn.add(six);
        ixs.map((ix)=>
        {
            txn.add(ix);
        });
        
        if(txn.instructions.length <= 1){
            return "SKIPPING TXN: NO UPDATES REQUIRED";
        } 

        const connection = this.mclient.connection;
        const payer = this.mclient.payer;
        txn.feePayer = payer.publicKey
        txn.recentBlockhash = (
            await connection.getRecentBlockhash("processed")
        ).blockhash;

        txn.partialSign(payer)
        const stxn = txn.serialize();
        const txh = await connection.sendRawTransaction(stxn, 
            {
                skipPreflight : true,
                preflightCommitment : "processed"
            });
        await connection.confirmTransaction(txh, "processed");
        return txh;
    }
    
}