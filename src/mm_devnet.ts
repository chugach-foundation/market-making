import { PublicKey, Keypair, Connection } from "@solana/web3.js";
import NodeWallet from "@project-serum/anchor/dist/cjs/nodewallet";
import { CypherMMClient } from "./mm_client";
import { loadPayer, FastTXNBuilder, getBalance } from "./utils";
import { MM_Strat } from "./strats/mmstrat";
import { TopOfBookStrat } from "./strats/top";
import {
	CONFIGS,
	Cluster,
	CypherClient,
	CypherGroup,
	CypherUserController,
} from "@chugach-foundation/cypher-client";
import { splToUiAmount } from "@chugach-foundation/cypher-client/lib/utils/tokenAmount";
import { BN } from "@project-serum/anchor";
import { ASSOCIATED_PROGRAM_ID, TOKEN_PROGRAM_ID } from "@project-serum/anchor/dist/cjs/utils/token";
import {Token} from "@solana/spl-token"

const USDC_MINT = new PublicKey("DPhNUKVhnrkdbq37GUgTUBRbZLsvziX1p5e5YUXyjBsb");

export const wait = (delayMS: number) =>
	new Promise((resolve) => setTimeout(resolve, delayMS));


const tenk = new BN(10000000000);
async function faucet_spam(ctr : CypherUserController, payer : Keypair, con : Connection){
	const builder = new FastTXNBuilder(payer, con);
	const ata = await Token.getAssociatedTokenAddress(
		ASSOCIATED_PROGRAM_ID,
		TOKEN_PROGRAM_ID,
		USDC_MINT,
		payer.publicKey
	  );
	const ix = await ctr.client.testDriver.getMintInstr(ata, tenk);
	while (true){
		for(let i = 0; i < 37; i++){
			builder.add(ix);
		}
		const {execute} = await builder.build();
		const txh = await execute();
		console.log(txh);
		builder.ixs = [];
	}
}


async function marketMaker() {
	let cluster: Cluster = "devnet";

	let bidk = loadPayer(process.env.CKEY ?? process.env.SECRET_KEY);
	let groupAddr: PublicKey = new PublicKey("7aDJqXVTexwugfKypP4zi4yncUkhoJDZLrZ2K9unRqu7");

	const bidclient = new CypherClient(cluster, new NodeWallet(bidk), { commitment: "processed", skipPreflight: true });
	let rpcAddy = 'https://psytrbhymqlkfrhudd.dev.genesysgo.net:8899/';

	const bidctr = await CypherUserController.loadOrCreate(bidclient, groupAddr);
	const group = await CypherGroup.load(bidclient, groupAddr);
	let cAssetMint = group.cAssetMints[5];
	let cAssetMarket = group.getDexMarket(cAssetMint).address;
	let programAddress = new PublicKey('DsGUdHQY2EnvWbN5VoSZSyjL4EWnStgaJhFDmJV34GQQ');

	const client = await CypherMMClient.load(
		cAssetMint,
		"devnet",
		rpcAddy,
		groupAddr,
		process.env.CKEY ?? process.env.SECRET_KEY,
		process.env.CKEYTWO ?? process.env.TEST_KEY
	);
	//await faucet_spam(client.bidctr, client.bidPayer, client.connection);
	const bidderInitOpenOrdersInstr = await client.bidctr.makeInitOpenOrdersInstr(cAssetMint);
	const minterInitOpenOrdersInstr = await client.mintctr.makeInitOpenOrdersInstr(cAssetMint);
	//console.log(client.mintPayer.publicKey.toString());
	if (bidderInitOpenOrdersInstr || minterInitOpenOrdersInstr) {
		
		
		const builder = new FastTXNBuilder(client.mintPayer, client.connection);
		if(bidderInitOpenOrdersInstr){
			builder.add(bidderInitOpenOrdersInstr);
			builder.singers.push(client.bidPayer)
		}
		if (minterInitOpenOrdersInstr){
			builder.add(minterInitOpenOrdersInstr);
			builder.singers.push(client.mintPayer);
		}
		const { execute } = await builder.build();
		const txh = await execute();
		let res = await client.connection.confirmTransaction(txh, "confirmed");
		if (res.value.err){
			throw new Error("Failed to create open orders accounts!!");
		}
		console.log("txh if initOpenOrdersInstr called: " + txh)
	}

	const bidder_usdc = await getBalance(USDC_MINT, bidk.publicKey, client.connection);
	const minter_usdc = await getBalance(USDC_MINT, client.mintPayer.publicKey, client.connection);
	const builder = new FastTXNBuilder(client.mintPayer, client.connection);
	if(bidder_usdc > new BN(1000000)){
		builder.add(await client.depositMarketBidCollateralInstr(cAssetMint, bidder_usdc));
		builder.singers.push(client.bidPayer);
	}
	if (minter_usdc > new BN(1000000)){
		builder.add(await client.depositMarketMintCollateralInstr(cAssetMint, minter_usdc));
		builder.singers.push(client.mintPayer);
	}
	if(builder.ixs.length){
		console.log("depositing collateral...");
		const {execute} = await builder.build();
		const txh = await execute();
		console.log(txh);
		const conf = await client.connection.confirmTransaction(txh, "confirmed");
		if(conf.value.err){
			console.log(conf.value.err);
			throw new Error("FAILED TO DEPOSIT COLLATERAL");
		}
	}
	const strat: MM_Strat = new TopOfBookStrat(client,
		{
			max_size: 10000, // increased size due to low price of sol/eth pair, probably should add market specific sizing
			time_requote: 1000
		})
	strat.start();
	console.log("Strat started on cAsset: " + cAssetMint)
}
marketMaker();
