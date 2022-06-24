use spl_math::uint::U192;

#[inline(always)]
pub fn convert_pc_to_lots(price: u64, pc_lot_size: u64) -> u64 {
    price / pc_lot_size
}

#[inline(always)]
pub fn convert_price_to_lots(
    price: u64,
    coin_lot_size: u64,
    coin_decimals_factor: u64,
    pc_lot_size: u64,
) -> u64 {
    (price * coin_lot_size) / (coin_decimals_factor * pc_lot_size)
}

#[inline(always)]
pub fn convert_base_to_lots(amount_in: u64, coin_lot_size: u64) -> u64 {
    amount_in / coin_lot_size
}

#[inline(always)]
pub fn convert_base_to_decimals(amount_in: u64, coin_lot_size: u64) -> u64 {
    amount_in * coin_lot_size
}

#[inline(always)]
pub fn convert_pc_to_decimals(price: u64, pc_lot_size: u64) -> u64 {
    //idk if overflow possible here so being safe
    price * pc_lot_size
}

#[inline(always)]
pub fn convert_price_to_decimals(
    price: u64,
    coin_lot_size: u64,
    coin_decimals_factor: u64,
    pc_lot_size: u64,
) -> u64 {
    let mid = U192::from(price);
    //idk if overflow possible here so being safe
    let res = mid * pc_lot_size * coin_decimals_factor / coin_lot_size;
    //This really shouldn't ever panic... but if it does change to U256..
    res.try_into().unwrap()
}
