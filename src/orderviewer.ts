import {
    PublicKey,
    Connection,
} from "@solana/web3.js";
import { Market } from "@project-serum/serum/lib/market";
import {LiveMarket } from "./livemarket/live_market";
import { Cluster, CONFIGS, CypherClient, CypherGroup, CypherUserController } from "@chugach-foundation/cypher-client";
import NodeWallet from "@project-serum/anchor/dist/cjs/nodewallet";
import { loadPayer } from "./utils";
const interval = 1000
const control = { isRunning: true, interval: interval };

let connection = new Connection('https://psytrbhymqlkfrhudd.dev.genesysgo.net:8899/', "processed");
    console.log('connection made: ' + connection.toString());

export const wait = (delayMS: number) =>
  new Promise((resolve) => setTimeout(resolve, delayMS));


async function getOrderbookData() {
    let market = await Market.load(connection, cAssetMint, {}, CONFIGS.devnet.DEX_PID);
    const livemarket = new LiveMarket(connection, market);
    await livemarket.start((info) => 
    {
        livemarket.printBook();
    });
    livemarket.printBook();
    while (true) {
        console.log("waiting");
        await wait(1000000);
    }
}

async function run{
    


    let cluster : Cluster = "devnet";
    let bidk = loadPayer(process.env.CKEY);
    let groupAddr : PublicKey;
    let cAssetMint : PublicKey = process.env.CASSETMINT ?? ;
    const bidclient = new CypherClient(cluster, new NodeWallet(bidk), { commitment: "processed", skipPreflight: true });
    const bidctr = await CypherUserController.loadOrCreate(bidclient, groupAddr);
    const group = await CypherGroup.load(bidclient, groupAddr);
    let cAssetMarket = new PublicKey("8eTZf8a3CUHkuNC9LtAvCnmiPJN5bh2hxk8cDg53vjQU")
    let programAddress = new PublicKey('DsGUdHQY2EnvWbN5VoSZSyjL4EWnStgaJhFDmJV34GQQ');
    console.log('cAssetMint: ' + cAssetMint.toString() + ' | ' + 'programAddress: ' + programAddress.toString());
    getOrderbookData();
}

