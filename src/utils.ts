import { Keypair } from "@solana/web3.js";

export const loadPayer = (keypairPath: string): Keypair => {
    return Keypair.fromSecretKey(
      Buffer.from(
        JSON.parse(
          require("fs").readFileSync(keypairPath, {
            encoding: "utf-8",
          })
        )
      )
    );
};

export const wait = (delayMS: number) =>
  new Promise((resolve) => setTimeout(resolve, delayMS));
