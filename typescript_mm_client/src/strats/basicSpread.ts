import { TransactionInstruction } from "@solana/web3.js"
import { Order } from "@project-serum/serum/lib/market";
import { CypherMMClient } from "src/mm_client";
import { MM_Strat } from "./mmstrat"
import { MM_Hyperparams } from "./stypes";
import { pythPriceStream } from "src/oracles/pythWS";
import {
    wait,
    FastTXNBuilder
} from "../utils";

export class basicSpread implements MM_Strat {
    mmclient: CypherMMClient;
    hparams: MM_Hyperparams;
    quoteStream: pythPriceStream;

    constructor(client: CypherMMClient, hparams: MM_Hyperparams, pythPriceStream) {
        this.mmclient = client;
        this.hparams = hparams;
        this.quoteStream = pythPriceStream
    }

    private async run(): Promise<void> {
        while (true) {
            try {
                console.log(await this.quoteSpread());
                await wait(this.hparams.time_requote);
            }
            catch (e) {
                console.error(e);
            }
        }
    }

    async start(): Promise<void> {
        this.run();
        console.log("Initialized Strategy: basic spread quoting");
    }

    private async quoteSpread() {
        const size = this.hparams.max_size;
        let qbid, qask;

        const priceFeed: pythPriceStream = new pythPriceStream('devnet', 'Crypto.SOL/USD')
        const price = await priceFeed.getPriceForQuotes()

        let [qbid, qask] = [priceFeed., task - .0001];
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

        singers.push(this.mmclient.traderk);
        const builder = new FastTXNBuilder(this.mmclient.traderk, this.mmclient.connection, singers);

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