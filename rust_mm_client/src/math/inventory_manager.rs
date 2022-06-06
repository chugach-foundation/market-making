use super::frac_maths::SimpleFracu32;
use fraction::ToPrimitive;

pub struct InventoryManager {
    max_quote: i64,
    shape: SimpleFracu32,
}
pub struct QuoteVolumes {
    bid_size: i64,
    ask_size: i64,
}

//Number we use here is arbitrary, shape mul can do conversion to any base..
const EXP_BASE: i64 = 3;

///WIP -- NEED TO FIGURE OUT SOME VALUE CONVERSION STUFF DONT USE YET
//Returned as
impl InventoryManager {
    pub fn get_quote_volumes(&self, current_delta: i64) -> QuoteVolumes {
        let adjusted_vol = self.adj_quote_size(current_delta.abs().try_into().unwrap());
        let (bid_size, ask_size) = if current_delta < 0 {
            (self.max_quote, adjusted_vol)
        } else {
            (adjusted_vol, self.max_quote)
        };
        QuoteVolumes { bid_size, ask_size }
    }

    fn adj_quote_size(&self, abs_delta: u32) -> i64 {
        self.max_quote / EXP_BASE.pow(self.shape * abs_delta)
    }
}
