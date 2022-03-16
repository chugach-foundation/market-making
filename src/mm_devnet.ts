import * as anchor from "@project-serum/anchor";
import { CAssetPubkeys, GroupPubkeys} from "@chugach-foundation/cypher-client";
import {PublicKey} from "@solana/web3.js";
import { CypherMMClient } from "./mm_client";
import { MM_Strat} from "./strats/mmstrat";
import { TopOfBookStrat } from "./strats/top";

export const wait = (delayMS: number) =>
  new Promise((resolve) => setTimeout(resolve, delayMS));

async function marketMaker() {
	const rpcAddy = "https://psytrbhymqlkfrhudd.dev.genesysgo.net:8899/"
	let cAssetMarket = new PublicKey("8eTZf8a3CUHkuNC9LtAvCnmiPJN5bh2hxk8cDg53vjQU");
	let programAddress = new PublicKey('DsGUdHQY2EnvWbN5VoSZSyjL4EWnStgaJhFDmJV34GQQ');
	let cAssetMint = CAssetPubkeys.DEVNET[0];


	const client = await CypherMMClient.load({
		cAssetMarketProgramAddress : programAddress,
		cAssetMint : cAssetMint,
		cAssetOrderbookAddress : cAssetMarket,
	},
	"DEVNET",
	rpcAddy,
	GroupPubkeys.DEVNET,
	process.env.TEST_KEY,
	process.env.SECRET_KEY
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
