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
} from "@chugach-foundation/cypher-client";
import { deriveOpenOrdersAddress, deriveMarketAuthority } from "@chugach-foundation/cypher-client/lib/utils";
import { BN } from "@project-serum/anchor";



export const wait = (delayMS: number) =>
    new Promise((resolve) => setTimeout(resolve, delayMS));


async function marketMaker() {
    const USDC_MINT = new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
    let rpcAddy = 'https://ssc-dao.genesysgo.net';
    const cluster: Cluster = 'mainnet-beta';
    let traderk = loadPayer(process.env.CMKEY ?? process.env.SECRET_KEY);
    let groupAddr: PublicKey = new PublicKey("BByD8HAf6mqRKZTryjywHG4FvawZKhrrdBjPfpRHYJnv");

    const client = new CypherClient(cluster, new NodeWallet(traderk), { commitment: "processed", skipPreflight: true });
    const traderctr = await CypherUserController.loadOrCreate(client, groupAddr);
    const group = await CypherGroup.load(client, groupAddr);

    // TODO: make selected market more dynamic
    let cAssetMint = group.cAssetMints[0];
    let cAssetMarket = group.getDexMarket(cAssetMint).address;

    if (!traderctr.user.address) {
        const [newAddr, bump] = await CypherUserController.deriveAddress(client, groupAddr);
        const initUserIx = await makeInitCypherUserIx(client, groupAddr, newAddr, bump);
        const tx = new Transaction();
        tx.add(initUserIx);
        await client.anchorProvider.sendAndConfirm(tx, [traderk]);
    }


    const mmclient = await CypherMMClient.load(
        cAssetMint,
        cluster,
        rpcAddy,
        group,
        traderctr.userController,
        traderk
    );
    //await faucet_spam(client.traderctr, client.bidPayer, client.connection);
    const [deriveOOAccount, bumpoo] = await deriveOpenOrdersAddress(cAssetMarket, traderctr.user.address, CONFIGS[cluster].CYPHER_PID);
    const ooAccount = await client.connection.getAccountInfo(deriveOOAccount, "processed");

    if (!ooAccount) {

        const traderInitOpenOrdersInstr = await makeInitOpenOrdersIx(mmclient.traderctr.user, cAssetMint);

        const builder = new FastTXNBuilder(traderk, mmclient.connection);
        if (traderInitOpenOrdersInstr) {
            builder.add(traderInitOpenOrdersInstr);
            builder.singers.push(traderk)
        }

        const { execute } = await builder.build();
        const txh = await execute();
        let res = await mmclient.connection.confirmTransaction(txh, "confirmed");
        if (res.value.err) {
            throw new Error("Failed to create open orders accounts!!");
        }
        console.log("txh if initOpenOrdersInstr called: " + txh)
    }

    const trader_usdc = await getBalance(USDC_MINT, traderk.publicKey, mmclient.connection);
    const builder = new FastTXNBuilder(traderk, mmclient.connection);
    // @ts-ignore
    builder.add(await makeDepositCollateralIx(client.trader, new BN(trader_usdc)));
    builder.singers.push(mmclient.traderk);


    if (builder.ixs.length) {
        console.log("depositing collateral...");
        const { execute } = await builder.build();
        const txh = await execute();
        console.log(txh);
        const conf = await mmclient.connection.confirmTransaction(txh, "confirmed");
        if (conf.value.err) {
            console.log(conf.value.err);
            throw new Error("FAILED TO DEPOSIT COLLATERAL");
        }
    }
    const strat: MM_Strat = new TopOfBookStrat(mmclient,
        {
            max_size: 10000, // increased size due to low price of sol/eth pair, probably should add market specific sizing
            time_requote: 1000
        })
    strat.start();
    console.log("Strat started on cAsset: " + cAssetMint)
}
marketMaker();
