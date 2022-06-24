use solana_sdk::{
    hash::Hash, instruction::Instruction, message::Message, signature::Keypair, signer::Signer,
    transaction::Transaction,
};

#[derive(Debug, Default)]
pub struct FastTxnBuilder {
    pub ixs: Vec<Instruction>,
}

impl FastTxnBuilder {
    pub fn new() -> FastTxnBuilder {
        FastTxnBuilder::default()
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.ixs.len()
    }

    #[inline(always)]
    pub fn add(&mut self, ix: Instruction) {
        self.ixs.push(ix);
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.ixs.clear();
    }

    pub fn build(
        &self,
        recent_blockhash: Hash,
        payer: &Keypair,
        additional_signers: Option<&Vec<Keypair>>,
    ) -> Transaction {
        let message = Message::new(&self.ixs[..], Some(&payer.pubkey()));
        let mut txn = Transaction::new_unsigned(message);
        txn.partial_sign(&[payer], recent_blockhash);
        if let Some(adsigners) = additional_signers {
            for adsigner in adsigners {
                txn.partial_sign(&[adsigner], recent_blockhash);
            }
        }
        txn
    }
}
