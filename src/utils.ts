import {
  Keypair,
  Transaction,
  TransactionInstruction,
  Connection,
  Signer
} from "@solana/web3.js"

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
    this.singers = signers;
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
      await this.connection.getRecentBlockhash("processed")
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

