import {CypherMMClient} from "../mm_client";
import { MM_Hyperparams } from "./stypes";

export interface MM_Strat{
    mclient : CypherMMClient
    hparams : MM_Hyperparams
    //async void very bad... revisit later
    start() : Promise<void>
};