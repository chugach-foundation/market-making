import { PublicKey, Keypair, Connection, Transaction } from "@solana/web3.js";
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
	makeInitOpenOrdersIx,
	makeDepositCollateralIx,
	makeInitCypherUserIx,
	CypherUser
} from "@chugach-foundation/cypher-client";
import { deriveOpenOrdersAddress, deriveMarketAuthority } from "@chugach-foundation/cypher-client/lib/utils";
import { BN } from "@project-serum/anchor";
import { ASSOCIATED_PROGRAM_ID, TOKEN_PROGRAM_ID } from "@project-serum/anchor/dist/cjs/utils/token";
import { Token } from "@solana/spl-token"

const USDC_MINT = new PublicKey("DPhNUKVhnrkdbq37GUgTUBRbZLsvziX1p5e5YUXyjBsb");
const cluster: Cluster = 'devnet';

export const wait = (delayMS: number) =>
	new Promise((resolve) => setTimeout(resolve, delayMS));


const tenk = new BN(10000000000);
async function faucet_spam(ctr: CypherUserController, payer: Keypair, con: Connection) {
	const builder = new FastTXNBuilder(payer, con);
	const ata = await Token.getAssociatedTokenAddress(
		ASSOCIATED_PROGRAM_ID,
		TOKEN_PROGRAM_ID,
		USDC_MINT,
		payer.publicKey
	);

	// @ts-ignore
	const ix = await ctr.client.testDriver.getMintInstr(ata, tenk);
	for (let i = 0; i < 10; i++) {
		builder.add(ix);
	}
	const { execute } = await builder.build();
	const txh = await execute();
	console.log(txh);
	builder.ixs = [];
}



async function marketMaker() {
	let traderk = loadPayer(process.env.CMKEY ?? process.env.SECRET_KEY);
	let groupAddr: PublicKey = new PublicKey("B9v8Nbd2X9UJmVF4ZSng1Nj6wQ9Q86LfEFUbWUw7E7XU");

	const tradeclient = new CypherClient(cluster, new NodeWallet(traderk), { commitment: "processed", skipPreflight: true });
	makeInitCypherUserIx
	let rpcAddy = 'https://devnet.genesysgo.net';

	const traderctr = await CypherUserController.loadOrCreate(tradeclient, groupAddr);
	const group = await CypherGroup.load(tradeclient, groupAddr);
	let cAssetMint = group.cAssetMints[0];
	let cAssetMarket = group.getDexMarket(cAssetMint).address;
	let programAddress = new PublicKey('DsGUdHQY2EnvWbN5VoSZSyjL4EWnStgaJhFDmJV34GQQ');

	if (!traderctr.user.address) {
		const [newAddr, bump] = await CypherUserController.deriveAddress(tradeclient, groupAddr);
		const initUserIx = await makeInitCypherUserIx(tradeclient, groupAddr, newAddr, bump);
		const tx = new Transaction();
		tx.add(initUserIx);
		await tradeclient.anchorProvider.sendAndConfirm(tx, [traderk]);
	}


	const client = await CypherMMClient.load(
		cAssetMint,
		"devnet",
		rpcAddy,
		group,
		traderctr.userController,
		traderk,
	);
	//await faucet_spam(client.traderctr, client.bidPayer, client.connection);
	const [deriveOOAccount, bumpoo] = await deriveOpenOrdersAddress(cAssetMarket, traderctr.user.address, CONFIGS[cluster].CYPHER_PID);
	const ooAccount = await tradeclient.connection.getAccountInfo(deriveOOAccount, "processed");

	if (!ooAccount) {

		const bidderInitOpenOrdersInstr = await makeInitOpenOrdersIx(client.traderctr.user, cAssetMint);

		const builder = new FastTXNBuilder(traderk, client.connection);
		if (bidderInitOpenOrdersInstr) {
			builder.add(bidderInitOpenOrdersInstr);
			builder.singers.push(traderk)
		}

		const { execute } = await builder.build();
		const txh = await execute();
		let res = await client.connection.confirmTransaction(txh, "confirmed");
		if (res.value.err) {
			throw new Error("Failed to create open orders accounts!!");
		}
		console.log("txh if initOpenOrdersInstr called: " + txh)
	}

	const bidder_usdc = await getBalance(USDC_MINT, traderk.publicKey, client.connection);
	const builder = new FastTXNBuilder(traderk, client.connection);
	if (bidder_usdc > new BN(1000000)) {
		// @ts-ignore
		builder.add(await makeDepositCollateralIx(client.trader, new BN(bidder_usdc)));
		builder.singers.push(client.traderk);
	}

	if (builder.ixs.length) {
		console.log("depositing collateral...");
		const { execute } = await builder.build();
		const txh = await execute();
		console.log(txh);
		const conf = await client.connection.confirmTransaction(txh, "confirmed");
		if (conf.value.err) {
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
