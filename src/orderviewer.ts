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

let connection = new Connection('https://psytrbhymqlkfrhudd.dev.genesysgo.net:8899/', "processed");
console.log('connection made: ' + connection.toString());

export const wait = (delayMS: number) =>
    new Promise((resolve) => setTimeout(resolve, delayMS));


async function getOrderbookData(cAssetMarket: PublicKey) {
    let market = await Market.load(connection, cAssetMarket, {}, CONFIGS.devnet.DEX_PID);
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

    let cluster: Cluster = "devnet";
    let bidk = loadPayer(process.env.CKEY);
    let groupAddr: PublicKey = new PublicKey("7aDJqXVTexwugfKypP4zi4yncUkhoJDZLrZ2K9unRqu7");

    const bidclient = new CypherClient(cluster, new NodeWallet(bidk), { commitment: "processed", skipPreflight: true });


    const bidctr = await CypherUserController.loadOrCreate(bidclient, groupAddr);
    const group = await CypherGroup.load(bidclient, groupAddr);
    let cAssetMint = group.cAssetMints[5];
    console.log(group.cAssetMints)
    let cAssetMarket = group.getDexMarket(cAssetMint).address;
    let programAddress = new PublicKey('DsGUdHQY2EnvWbN5VoSZSyjL4EWnStgaJhFDmJV34GQQ');
    console.log('cAssetMint: ' + cAssetMint.toString() + ' | ' + 'programAddress: ' + programAddress.toString());
    console.log(`Market address : ${cAssetMarket.toString()}`)
    getOrderbookData(cAssetMarket);
}
run();
