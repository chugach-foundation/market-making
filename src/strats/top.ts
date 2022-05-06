import { TransactionInstruction } from "@solana/web3.js"
import { Order } from "@project-serum/serum/lib/market";
import { CypherMMClient } from "src/mm_client";
import { MM_Strat } from "./mmstrat"
import { MM_Hyperparams } from "./stypes";
import {
    wait,
    FastTXNBuilder
} from "../utils";
import { createEmitAndSemanticDiagnosticsBuilderProgram } from "typescript";

export class TopOfBookStrat implements MM_Strat {
    mclient: CypherMMClient;
    hparams: MM_Hyperparams;

    constructor(client: CypherMMClient, hparams: MM_Hyperparams) {
        this.mclient = client;
        this.hparams = hparams;
    }

    private async run(): Promise<void> {
        while (true) {
            try {
                console.log(await this.quoteTop());
                await wait(this.hparams.time_requote);
            }
            catch (e) {
                console.error(e);
            }
        }
    }

    async start(): Promise<void> {
        this.run();
        console.log("Initialized Strategy: Top Of Book");
    }

    private async quoteTop() {
        const size = this.hparams.max_size;
        let tbid, task;
        try {
            [tbid, task] = this.mclient.getTopSpread();
        }
        catch (e) {
            [tbid, task] = [1, 1000000];
        }

        let [qbid, qask] = [tbid + .001, task - .001];
        if (qbid >= qask) {
            qbid -= .001;
            qask += .001
        }

        const singers = []
        let bidix: TransactionInstruction;
        let depositMktix: TransactionInstruction;
        let mintix: TransactionInstruction;
        //const mintix = this.mclient.makeAskInstruction(qask, size);

        //TODO -- Modify this segment to avoid checking if buy and if sell now that we've separated into two accounts
        let oinfo = await this.mclient.getOutOrdersInfo(this.mclient.bidctr);
        const bidsize = size, asksize = size;

        let bidsToCancel: Order[] = []
        let mintsToCancel: Order[] = []

        if (oinfo.bidPrice == qbid || oinfo.bidPrice == qbid - .001) {
            //Don't frontrun self, but check if we need to reinforce the order
            //Add the fraction missing cuz queue priority
            qbid = oinfo.bidPrice;
            const dif = bidsize - oinfo.bidSize;
            if (dif > .1) {
                bidix = await this.mclient.makeBidInstruction(qbid, dif);
                //singers.push(this.mclient.bidPayer);
            }
        }
        else {
            bidix = await this.mclient.makeBidInstruction(qbid, bidsize);
            //singers.push(this.mclient.bidPayer);
            oinfo.orders.map(
                (order) => {
                    order.side == "buy" ? bidsToCancel.push(order) : {};
                }
            )
        }
        oinfo = await this.mclient.getOutOrdersInfo(this.mclient.mintctr);
        if (oinfo.askPrice == qask || oinfo.askPrice == qask + .001) {
            //Don't frontrun self, but check if we need to reinforce the order
            //Add the fraction missing cuz queue priority
            qask = oinfo.askPrice;
            const dif = asksize - oinfo.askSize;
            if (dif > .1) {
                depositMktix = await this.mclient.depositMintCollateralInstruction(qask, dif);
                mintix = await this.mclient.makeMintInstruction(qask, dif);
            }
        }
        else {
            depositMktix = await this.mclient.depositMintCollateralInstruction(qask, asksize);
            mintix = await this.mclient.makeMintInstruction(qask, asksize);
            oinfo.orders.map(
                (order) => {
                    order.side == "sell" ? mintsToCancel.push(order) : {};
                }
            )
        }

        singers.push(this.mclient.mintPayer);
        singers.push(this.mclient.bidPayer);
        const builder = new FastTXNBuilder(this.mclient.mintPayer, this.mclient.connection, singers);

        if (bidsToCancel.length) builder.add(await this.mclient.makeCancelBidOrdersInstructions(bidsToCancel));
        if (mintsToCancel.length) builder.add(await this.mclient.makeCancelMintOrdersAndWithdrawMktCollateralInstructions(mintsToCancel));
        if (bidix) builder.add(bidix);
        if (depositMktix) builder.add(depositMktix);
        if (mintix) builder.add(mintix);
        const six = await this.mclient.makeSettleFundsInstruction();
        builder.add(six);

        if (builder.ixs.length <= 2) {
            return "SKIPPING TXN: NO UPDATES REQUIRED";
        }

        const { execute } = await builder.build();
        const txh = await execute();
        await this.mclient.connection.confirmTransaction(txh, "processed");
        return txh;
    }
}