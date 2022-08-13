use {
    solana_account_decoder::{UiAccountEncoding, UiAccount},
    crate::{MarketMakerError}
};

pub fn get_account_info(account: &UiAccount) -> Result<Vec<u8>, MarketMakerError> {
    let (ai, enc) = match &account.data {
        solana_account_decoder::UiAccountData::Binary(s, e) => (s, *e),
        _ => return Err(MarketMakerError::InvalidAccountResponseFormat)
    };

    if enc != UiAccountEncoding::Base64 {
        return Err(MarketMakerError::InvalidAccountDataEncoding);
    }

    let account_data_res = base64::decode(ai);
    match account_data_res {
        Ok(a) => Ok(a),
        Err(e) => return Err(MarketMakerError::AccountInfoDecoding(e)),
    }
}