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
    mmclient: CypherMMClient;
    hparams: MM_Hyperparams;

    constructor(client: CypherMMClient, hparams: MM_Hyperparams) {
        this.mmclient = client;
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
            [tbid, task] = this.mmclient.getTopSpread();
        }
        catch (e) {
            [tbid, task] = [1, 1000000];
        }

        let [qbid, qask] = [tbid + .0001, task - .0001];
        if (qbid >= qask) {
            qbid -= .0001;
            qask += .0001
        }

        const singers = []
        let bidix: TransactionInstruction;
        let sellix: TransactionInstruction;
        //let depositMktix: TransactionInstruction;

        //TODO -- Modify this segment to avoid checking if buy and if sell now that we've separated into two accounts
        let oinfo = await this.mmclient.getOutOrdersInfo(this.mmclient.traderctr);
        const bidsize = size, asksize = size;

        let ordersToCancel: Order[] = []

        if (oinfo.bidPrice == qbid || oinfo.bidPrice == qbid - .0001) {
            //Don't frontrun self, but check if we need to reinforce the order
            //Add the fraction missing cuz queue priority
            qbid = oinfo.bidPrice;
            const dif = bidsize - oinfo.bidSize;
            if (dif > .1) {
                bidix = await this.mmclient.placeOrderIx(qbid, dif, "buy");
                //singers.push(this.mclient.bidPayer);
            }
        }
        else {
            bidix = await this.mmclient.placeOrderIx(qbid, bidsize, "buy");
            //singers.push(this.mclient.bidPayer);
            oinfo.orders.map(
                (order) => {
                    order.side == "buy" ? ordersToCancel.push(order) : {};
                }
            )
        }

        if (oinfo.askPrice == qask || oinfo.askPrice == qask + .0001) {
            //Don't frontrun self, but check if we need to reinforce the order
            //Add the fraction missing cuz queue priority
            qask = oinfo.askPrice;
            const dif = asksize - oinfo.askSize;
            if (dif > .1) {
                //depositMktix = await this.mclient.depositMintCollateralInstruction(qask, dif);
                sellix = await this.mmclient.placeOrderIx(qask, dif, "sell");
            }
        }
        else {
            //depositMktix = await this.mclient.depositMintCollateralInstruction(qask, asksize);
            sellix = await this.mmclient.placeOrderIx(qask, asksize, "sell");
            oinfo.orders.map(
                (order) => {
                    order.side == "sell" ? ordersToCancel.push(order) : {};
                }
            )
        }

        singers.push(this.mmclient.bidPayer);
        const builder = new FastTXNBuilder(this.mmclient.bidPayer, this.mmclient.connection, singers);

        if (ordersToCancel.length) builder.add(await this.mmclient.cancelOrderIx(ordersToCancel));
        if (bidix) builder.add(bidix);
        if (sellix) builder.add(sellix);
        //if (depositMktix) builder.add(depositMktix);
        const six = await this.mmclient.settleFundsIx();
        builder.add(six);

        if (builder.ixs.length <= 2) {
            return "SKIPPING TXN: NO UPDATES REQUIRED";
        }

        const { execute } = await builder.build();
        const txh = await execute();
        await this.mmclient.connection.confirmTransaction(txh, "processed");
        return txh;
    }
}