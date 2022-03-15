import * as anchor from "@project-serum/anchor";
import { connection, sleep } from "@project-serum/common";
import { CypherClient, CONFIGS, CAssetPubkeys, GroupPubkeys} from "@chugach-foundation/cypher-client";
import {PublicKey} from "@solana/web3.js";
import {Market} from "@project-serum/serum"
import { LiveMarket } from "./livemarket/live_market";
import {CypherUserController} from "./mmuser"
import {Transaction} from "@solana/web3.js"

export const wait = (delayMS: number) =>
  new Promise((resolve) => setTimeout(resolve, delayMS));

async function mint(
	minter: CypherUserController,
	cAssetMint: anchor.web3.PublicKey,
	price: number,
	size: number
) {
	return await minter.faucetUSDC(10000);
	//await minter.depositCollateral(cAssetMint, price * size * 5);
	//return await minter.mintCAssets(cAssetMint, size, price);
}

async function buy(
	trader: CypherUserController,
	cAssetMint: anchor.web3.PublicKey,
	price: number,
	size: number
) {
    await trader.faucetUSDC(price * size * 2);
    await trader.depositUSDCToMarginAccount(price * size * 2);
	await trader.placeOrderAndSettle(cAssetMint, {
		side: "buy",
		price,
		size,
		orderType: "limit",
		selfTradeBehavior: "decrementTake",
	});
}

async function quoteTop(lmarket : LiveMarket, provider : anchor.Provider, ctr : CypherUserController, mint : PublicKey){
	const size = 100;
	let tbid, task;
	try{
		[tbid, task] = lmarket.getTopSpread();
	}
	catch(e){
		[tbid, task] = [1, 1000000];
	}
	
	let [qbid, qask] = [tbid + .01, task - .01];
	if(qbid >= qask){
		qbid -=.01;
		qask +=.01
	}
	//inefficient af... fix
	const toCancel = await ctr.user.getMarketOrders(ctr.client, mint);
	console.log(toCancel.length);
	const ixs = []
	const ixs2 = []
	const bidix = ctr.makePlaceOrderInstr(mint, 
		{
			side : "buy",
			orderType : "postOnly",
			price : qbid,
			size : size,
			selfTradeBehavior : "decrementTake"
		})
	const mintix = ctr.makeMintCAssetsInstr(mint, size, qask);
	ixs.push(bidix, mintix)
	toCancel.map(
		(order) =>
	{
		ixs2.push(
			ctr.makeCancelOrderInstr(mint, order)
		)
	});
	const six = ctr.makeSettleFundsInstr(mint);
	
	const txn = new Transaction();
	//const txn2 = new Transaction();
	ixs2.map((ix)=>
	{
		txn.add(ix);
	});
	txn.add(six);
	ixs.map((ix)=>
	{
		txn.add(ix);
	});
	
	
	//provider.send(txn2);
	return provider.send(txn);
}

async function marketMaker() {
	const provider = anchor.Provider.local("https://psytrbhymqlkfrhudd.dev.genesysgo.net:8899/", {
		commitment: "processed",
		skipPreflight : true
	});
	let cAssetMarket = new PublicKey("8eTZf8a3CUHkuNC9LtAvCnmiPJN5bh2hxk8cDg53vjQU");

	let programAddress = new PublicKey('DsGUdHQY2EnvWbN5VoSZSyjL4EWnStgaJhFDmJV34GQQ');
	const connection = provider.connection
	let market = await Market.load(connection, cAssetMarket, {skipPreflight : true}, programAddress);
	//market.cancelOrder()
	//const live = new LiveMarket(provider.connection, market);
    let cont = await CypherUserController.load(new CypherClient("DEVNET", provider.wallet, provider.opts), GroupPubkeys.DEVNET, "ACCOUNT");
	let cAssetMint = CAssetPubkeys.DEVNET[0];
	//await live.start((info) => {});
	console.log(provider.wallet.publicKey.toString());
	//console.log("minting...");
	while(true){
		console.log(mint(cont, cAssetMint, 1500, 10));
		await wait(100);
	}
	
	/*console.log(await cont.placeOrder(cAssetMint, 
		{
			side : "sell",
			orderType : "limit",
			price : 1,
			size : 20,
			selfTradeBehavior : "decrementTake"
		}));
		*/
	
	/*while(true){
		try{

		
		console.log(await quoteTop(live, provider, cont, cAssetMint));
		await wait(10000);
		}
		catch(e){
			console.error(e);
		}
	}
	*/
	
	
}

marketMaker();
