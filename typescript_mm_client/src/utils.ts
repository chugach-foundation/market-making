import {
  Keypair,
  Transaction,
  PublicKey,
  TransactionInstruction,
  Connection,
  Signer
} from "@solana/web3.js"
import {Token, u64} from "@solana/spl-token"
import { ASSOCIATED_PROGRAM_ID, TOKEN_PROGRAM_ID } from "@project-serum/anchor/dist/cjs/utils/token";
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

//Build fast txns... maybe make more customizable later
export class FastTXNBuilder {

  payer: Keypair
  ixs: TransactionInstruction[]
  connection: Connection
  singers: Signer[]
  constructor(payer: Keypair, connection: Connection, signers?: Signer[]) {
    this.payer = payer;
    this.ixs = [];
    this.connection = connection;
    this.singers = signers ?? [];
  }

  add(ix: TransactionInstruction | TransactionInstruction[]) {
    const ixAdd = [].concat(ix);
    ixAdd.forEach(
      (ix) => {
        this.ixs.push(ix);
      }
    );
  }

  async build(): Promise<{ txn: Transaction, execute: () => Promise<string> }> {
    const txn = new Transaction();
    this.ixs.forEach(
      (ix) => {
        txn.add(ix);
      }
    )
    txn.feePayer = this.payer.publicKey;
    txn.recentBlockhash = (
      await this.connection.getLatestBlockhash("processed")
    ).blockhash;
    txn.partialSign(this.payer);
    if (this.singers) {
      this.singers.forEach(
        (singer) => {
          txn.partialSign(singer);
        }
      );
    }

    const stxn = txn.serialize();

    return {
      txn: txn,
      execute: () => {
        return this.connection.sendRawTransaction(stxn,
          {
            skipPreflight: true
          })
      }
    }

  }

}

export async function getBalance(tokenMint : PublicKey, owner : PublicKey, con : Connection){
  const ta = await Token.getAssociatedTokenAddress(
    ASSOCIATED_PROGRAM_ID,
    TOKEN_PROGRAM_ID,
    tokenMint,
    owner
  );
  const acc = await con.getAccountInfo(ta);
  return tokenAmountAccessor(acc)
}
export function tokenAmountAccessor(tokenAccountInfo) {
  return u64.fromBuffer(tokenAccountInfo.data.slice(64, 64 + 8));
}


