use ethereum_tx_sign::{
    web3::{
        api::Web3,
        futures::future::Future,
        transports::Http,
        types::{Address, TransactionReceipt, H256, U256},
    },
    RawTransaction,
};

use std::ops::Mul;
use std::str::FromStr;
use std::time::Duration;

// TODO: Move to lib.rs
pub struct EthereumSettlementEngine<S> {
    pub endpoint: String,
    pub signer: S,
    pub address: Address,
    pub chain_id: u8,
    pub confirmations: usize,
    pub poll_frequency: Duration,
}

impl<S: TxSigner + Send + Sync + 'static> EthereumSettlementEngine<S> {
    pub fn new(
        endpoint: String,
        signer: S,
        address: Address,
        chain_id: u8,
        confirmations: usize,
        poll_frequency: Duration,
    ) -> Self {
        EthereumSettlementEngine {
            endpoint,
            signer,
            address,
            chain_id,
            confirmations,
            poll_frequency,
        }
    }

    /// this should return a future that will be chained with the message call to
    /// the connector
    /// amount in wei
    pub fn settle_to(
        self,
        to: Address,
        amount: U256,
        // ) {
    ) -> impl Future<Item = TransactionReceipt, Error = ()> {
        let (_eloop, transport) = Http::new(&self.endpoint).unwrap();
        let web3 = Web3::new(transport);

        // get the nonce
        web3.eth()
            .transaction_count(self.address.clone(), None)
            .map_err(move |err| println!("Couldn't fetch nonce. Got error: {:#?}", err))
            .and_then(move |nonce| {
                // create the transaction
                let tx = make_tx(to, U256::from(amount), nonce);
                // sign the transaction
                let signed_tx = self.signer.sign(tx, &1);
                // send the transaction
                web3.send_raw_transaction_with_confirmation(
                    signed_tx.into(),
                    self.poll_frequency,
                    self.confirmations,
                )
                .map_err(move |err| println!("Could not submit raw transaction. Error: {:#?}", err))
            })
    }
}

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

fn to_wei(src: U256) -> U256 {
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