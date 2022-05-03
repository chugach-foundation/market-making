import * as anchor from "@project-serum/anchor";
import {Cluster, CONFIGS, CypherClient, CypherUserController, TokenAmount} from "@chugach-foundation/cypher-client";
import {PublicKey} from "@solana/web3.js";
import { CypherMMClient } from "./mm_client";
import { MM_Strat} from "./strats/mmstrat";
import { TopOfBookStrat } from "./strats/top";
import { CypherGroup } from "@chugach-foundation/cypher-client";
import { loadPayer } from "./utils";
import NodeWallet from "@project-serum/anchor/dist/cjs/nodewallet";

export const wait = (delayMS: number) =>
  new Promise((resolve) => setTimeout(resolve, delayMS));

async function marketMaker() {
	let cluster : Cluster = "devnet";

    let bidk = loadPayer(process.env.CKEY);
    let groupAddr : PublicKey = new PublicKey("7aDJqXVTexwugfKypP4zi4yncUkhoJDZLrZ2K9unRqu7");
    
    const bidclient = new CypherClient(cluster, new NodeWallet(bidk), { commitment: "processed", skipPreflight: true });
    let rpcAddy = 'https://psytrbhymqlkfrhudd.dev.genesysgo.net:8899/';
    
    const bidctr = await CypherUserController.loadOrCreate(bidclient, groupAddr);
    const group = await CypherGroup.load(bidclient, groupAddr);
    let cAssetMint = group.cAssetMints[5];
    console.log(group.cAssetMints)
    let cAssetMarket = group.getDexMarket(cAssetMint).address;
    let programAddress = new PublicKey('DsGUdHQY2EnvWbN5VoSZSyjL4EWnStgaJhFDmJV34GQQ');
	
	const client = await CypherMMClient.load(
	cAssetMint,
	"devnet",
	rpcAddy,
	groupAddr,
	process.env.CKEY,
	process.env.CKEYTWO
	);


	const strat : MM_Strat = new TopOfBookStrat(client,
		{
			max_size : 100,
			time_requote: 10000
		})
	strat.start();
	console.log("Strat started")
}

marketMaker();
