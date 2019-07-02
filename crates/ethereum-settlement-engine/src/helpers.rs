use ethereum_tx_sign::{
    web3::types::{Address, H256, U256},
    RawTransaction,
};
use std::ops::Mul;
use std::str::FromStr;

/// Trait whcih can be implemented for other types such as HSMs to be used with
/// the SE.
pub trait TxSigner {
    /// Takes a transaction and returns an RLP encoded signed version of it
    fn sign(&self, tx: RawTransaction, chain_id: &u8) -> Vec<u8>;
}

impl TxSigner for &str {
    fn sign(&self, tx: RawTransaction, chain_id: &u8) -> Vec<u8> {
        tx.sign(&H256::from_str(self).unwrap(), chain_id)
    }
}

pub fn to_wei(src: U256) -> U256 {
    let wei_in_ether = U256::from_dec_str("1000000000000000000").unwrap();
    src.mul(wei_in_ether)
}

// make signer class whcih takes a key and can do offline signing

pub fn make_tx(to: Address, value: U256, nonce: U256) -> RawTransaction {
    RawTransaction {
        to: Some(to),
        nonce,
        data: vec![],
        gas: 21000.into(),
        gas_price: 20000.into(),
        value,
    }
}