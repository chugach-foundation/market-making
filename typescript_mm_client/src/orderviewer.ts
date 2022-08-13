import {
    PublicKey,
    Connection,
} from "@solana/web3.js";
import NodeWallet from "@project-serum/anchor/dist/cjs/nodewallet";
import { Market } from "@project-serum/serum/lib/market";
import { loadPayer } from "./utils";
import { LiveMarket } from "./livemarket/live_market";
import {
    Cluster,
    CONFIGS,
    CypherClient,
    CypherGroup,
    CypherUserController
} from "@chugach-foundation/cypher-client";

const interval = 1000
const control = { isRunning: true, interval: interval };

let connection = new Connection('https://ssc-dao.genesysgo.net', "processed");
console.log('connection made: ' + connection.toString());

export const wait = (delayMS: number) =>
    new Promise((resolve) => setTimeout(resolve, delayMS));


async function getOrderbookData(cAssetMarket: PublicKey) {
    let market = await Market.load(connection, cAssetMarket, {}, CONFIGS["mainnet-beta"].DEX_PID);
    const livemarket = new LiveMarket(connection, market);
    await livemarket.start((info) => {
        livemarket.printBook();
    });
    livemarket.printBook();
    while (true) {
        console.log("waiting");
        await wait(1000000);
    }
}

async function run() {

    let cluster: Cluster = "mainnet-beta";
    let traderk = loadPayer(process.env.CMKEY);
    let groupAddr: PublicKey = new PublicKey("BByD8HAf6mqRKZTryjywHG4FvawZKhrrdBjPfpRHYJnv");

    const tradeclient = new CypherClient(cluster, new NodeWallet(traderk), { commitment: "processed", skipPreflight: true });


    const traderctr = await CypherUserController.loadOrCreate(tradeclient, groupAddr);
    const group = await CypherGroup.load(tradeclient, groupAddr);
    let cAssetMint = group.cAssetMints[0];
    let cAssetMarket = group.getDexMarket(cAssetMint).address;
    let programAddress = CONFIGS["mainnet-beta"].DEX_PID;
    console.log('cAssetMint: ' + cAssetMint.toString() + ' | ' + 'programAddress: ' + programAddress.toString());
    console.log(`Market address : ${cAssetMarket.toString()}`)
    getOrderbookData(cAssetMarket);
}
run();
