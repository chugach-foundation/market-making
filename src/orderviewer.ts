import {
    PublicKey,
    Connection,
} from "@solana/web3.js";
import { Market } from "@project-serum/serum/lib/market";
import {LiveMarket } from "./livemarket/live_market";
const interval = 1000
const control = { isRunning: true, interval: interval };

let connection = new Connection('https://psytrbhymqlkfrhudd.dev.genesysgo.net:8899/', "processed");
console.log('connection made: ' + connection.toString())

let cAssetMint = new PublicKey("8eTZf8a3CUHkuNC9LtAvCnmiPJN5bh2hxk8cDg53vjQU")
let programAddress = new PublicKey('DsGUdHQY2EnvWbN5VoSZSyjL4EWnStgaJhFDmJV34GQQ');
console.log('cAssetMint: ' + cAssetMint.toString() + ' | ' + 'programAddress: ' + programAddress.toString());

export const wait = (delayMS: number) =>
  new Promise((resolve) => setTimeout(resolve, delayMS));


async function getOrderbookData() {
    let market = await Market.load(connection, cAssetMint, {}, programAddress);
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

getOrderbookData();