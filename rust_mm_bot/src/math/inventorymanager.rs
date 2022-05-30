use std::f64::consts::E;
struct InventoryManager{
    max_quote : f32,
    shape: f32
}

struct QuoteVolumes{
    bid_size : u64,
    ask_size : u64
}

///WIP -- NEED TO FIGURE OUT SOME VALUE CONVERSION STUFF DONT USE YET
impl InventoryManager{
    pub fn get_quote_volumes(&self, current_delta : u64){

    }

    fn adj_quote_size(&self, abs_delta : u64) -> u64{
        u64::from(self.max_quote*E.powf(-abs_delta*shape));
    }
}
