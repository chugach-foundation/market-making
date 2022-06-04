import { Market, Orderbook } from "@project-serum/serum";
import { AccountChangeCallback } from "@solana/web3.js";

export type OrderBookInfo = {
  asks? : Orderbook
  bids? : Orderbook
}

export const handleBids =
  (
    market: Market,
    orderBookInfo: OrderBookInfo,
    handleUpdate: (orderBookInfo: OrderBookInfo) => void
  ): AccountChangeCallback =>
  (accountInfo, context) => {
    orderBookInfo.bids = Orderbook.decode(market, accountInfo.data);
    handleUpdate(orderBookInfo);
  };

export const handleAsks =
  (
    market: Market,
    orderBookInfo: OrderBookInfo,
    handleUpdate: (orderBookInfo: OrderBookInfo) => void
  ): AccountChangeCallback =>
  (accountInfo, context) => {
    orderBookInfo.asks = Orderbook.decode(market, accountInfo.data);
    handleUpdate(orderBookInfo);
  };
