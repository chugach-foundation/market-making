import * as anchor from "@project-serum/anchor";
import {CONFIGS} from "@chugach-foundation/cypher-client";
import {PublicKey} from "@solana/web3.js";
import { CypherMMClient } from "./mm_client";
import { MM_Strat} from "./strats/mmstrat";
import { TopOfBookStrat } from "./strats/top";
import { CypherGroup } from "@chugach-foundation/cypher-client";

export const wait = (delayMS: number) =>
  new Promise((resolve) => setTimeout(resolve, delayMS));

async function marketMaker() {
	const rpcAddy = "https://psytrbhymqlkfrhudd.dev.genesysgo.net:8899/"
	let cAssetMint = CONFIGS.devnet.;

	const client = await CypherMMClient.load(
	cAssetMint,
	"devnet",
	rpcAddy,
	,
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
