use ethabi::Token;
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

#[allow(unused)]
pub fn to_wei(src: U256) -> U256 {
    let wei_in_ether = U256::from_dec_str("1000000000000000000").unwrap();
    src.mul(wei_in_ether)
}

pub fn make_tx(
    to: Address,
    value: U256,
    nonce: U256,
    token_address: Option<Address>,
) -> RawTransaction {
    if let Some(token_address) = token_address {
        // erc20 function selector
        let mut data = hex::decode("a9059cbb").unwrap();
        data.extend(ethabi::encode(&vec![
            Token::Address(to),
            Token::Uint(value),
        ]));
        RawTransaction {
            to: Some(token_address),
            nonce,
            data: data,
            gas: 70000.into(), // ERC20 transactions cost approximately 40k gas.
            gas_price: 20000.into(),
            value: U256::zero(),
        }
    } else {
        RawTransaction {
            to: Some(to),
            nonce,
            data: vec![],
            gas: 21000.into(),
            gas_price: 20000.into(),
            value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_erc20_make_tx() {
        // https://etherscan.io/tx/0x6fd1b68f02f4201a38662647b7f09170b159faec6af4825ae509beefeb8e8130
        let to = "c92be489639a9c61f517bd3b955840fa19bc9b7c".parse().unwrap();
        let value = "16345785d8a0000".into();
        let nonce = 1.into();
        let token_address = Some("B8c77482e45F1F44dE1745F52C74426C631bDD52".into());
        let tx = make_tx(to, value, nonce, token_address);
        assert_eq!(hex::encode(tx.data), "a9059cbb000000000000000000000000c92be489639a9c61f517bd3b955840fa19bc9b7c000000000000000000000000000000000000000000000000016345785d8a0000")
    }

}
