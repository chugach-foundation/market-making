import {
  AccountChangeCallback,
  Connection
} from "@solana/web3.js";
import { Market } from "@project-serum/serum";

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
