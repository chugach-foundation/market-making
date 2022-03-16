import {Market, Orderbook} from "@project-serum/serum"
import { InferencePriority } from "typescript"
import {listenToAsks, listenToBids} from "./serumlisteners"
import {handleAsks, OrderBookInfo, handleBids} from "./orderbookhandlers"
import{Connection, AccountChangeCallback} from "@solana/web3.js"

export class LiveMarket
{
    private info : OrderBookInfo
    market: Market
    private connection : Connection
    bids: Orderbook
    constructor(connection : Connection, market : Market){
        this.connection = connection;
        this.market = market;  
    }

    private async prePopulate(){
        const [bids, asks] = await Promise.all([this.market.loadBids(this.connection), this.market.loadAsks(this.connection)]);
        this.info = {bids, asks}
    }

    

    printBook(){
        let [bids, asks] = [this.getBids(), this.getAsks()]
    // full orderbook data
    for (let order of bids) {
        console.log('orderID: ' + order.orderId
            + ' | price: ' + order.price
            + ' | size: ' + order.size
            + ' | side: ' + order.side);
    }

    console.log('-------------------------------------------------');

    // Full orderbook data
    for (let order of asks) {
        console.log('orderID: ' + order.orderId
            + ' | price: ' + order.price
            + ' | size: ' + order.size
            + ' | side: ' + order.side);
    }
    console.log('-------------------------------------------------');
    }

    async start(acb : (info: OrderBookInfo) => void){
        await this.prePopulate();
        listenToAsks(this.connection, this.market, handleAsks(this.market, this.info, acb));
        listenToBids(this.connection, this.market, handleBids(this.market, this.info, acb));
    }
    
    getAsks(){
        return this.info.asks;
    }

    getBids(){
        return this.info.bids;
    }

    getTopSpread(){
        return [this.getTopBid(), this.getTopAsk()];
    }

    getTopAsk(){
        return this.getAsks().getL2(1)[0][0];
    }

    getTopBid(){
        return this.getBids().getL2(1)[0][0];
    }
}