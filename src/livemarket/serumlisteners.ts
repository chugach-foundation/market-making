import { Market } from "@project-serum/serum";
import { AccountChangeCallback, Connection } from "@solana/web3.js";


export const listenToBids = (
  connection: Connection,
  market: Market,
  onUpdate: AccountChangeCallback
) => {
  connection.onAccountChange(market.bidsAddress, onUpdate, "processed");
};


export const listenToAsks = (
  connection: Connection,
  market: Market,
  onUpdate: AccountChangeCallback
) => {
  connection.onAccountChange(market.asksAddress, onUpdate, "processed");
};
