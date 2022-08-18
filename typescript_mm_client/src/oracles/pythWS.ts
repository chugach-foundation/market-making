
import { Connection, FetchMiddleware, PublicKey } from '@solana/web3.js'
import {
    PythConnection,
    getPythProgramKeyForCluster, PriceStatus
} from "@pythnetwork/client";
import {
    getPythClusterApiUrl,
    PythCluster
} from "@pythnetwork/client/lib/cluster";



export class pythPriceStream {
    private pythCluster: PythCluster
    pythSymbol: string
    private feed

    constructor(pythCluster: PythCluster, pythSymbol: string) {
        this.pythCluster = pythCluster;
        this.pythSymbol = pythSymbol;
    }



    private async streamPrice() {
        const SOLANA_CLUSTER_NAME: PythCluster = this.pythCluster
        const connection = new Connection(getPythClusterApiUrl(SOLANA_CLUSTER_NAME))
        const pythPublicKey = getPythProgramKeyForCluster(SOLANA_CLUSTER_NAME)

        const pythConnection = new PythConnection(connection, pythPublicKey)
        pythConnection.onPriceChange((product, price) => {

            if (price.price && price.confidence) {
                if (product.symbol === this.pythSymbol) {
                    // console.log(`${product.symbol}: $${price.price} \xB1$${price.confidence}`)
                    return [price.price, price.exponent]
                }
            } else {
                if (product.symbol === this.pythSymbol) {
                    // console.log(`${product.symbol}: price currently unavailable. status is ${PriceStatus[price.status]}`)
                    return PriceStatus[price.status]
                }
            }
        })

        // tslint:disable-next-line:no-console
        console.log('Reading from Pyth price feed...')
        pythConnection.start()

    }


    async getPriceForQuotes() {
        await this.streamPrice()
    }
}