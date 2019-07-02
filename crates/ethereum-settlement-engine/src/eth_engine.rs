use ethereum_tx_sign::web3::{
    api::Web3,
    futures::future::{ok, Future},
    transports::Http,
    types::{Address, TransactionReceipt, U256},
};
use hyper::{Response, StatusCode};
use std::time::Duration;

use crate::{make_tx, TxSigner};

// TODO: Move to lib.rs
pub struct EthereumSettlementEngine<S> {
    pub endpoint: String,
    pub signer: S,
    pub address: Address,
    pub chain_id: u8,
    pub confirmations: usize,
    pub poll_frequency: Duration,
}

impl_web! {
    impl<S> EthereumSettlementEngine<S>
    where
        S: TxSigner + Send + Sync + 'static
    {
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

        // this should return a future that will be chained with the message call to
        // the connector amount in wei
        pub fn settle_to(
            self,
            to: Address,
            amount: U256,
        ) -> impl Future<Item = TransactionReceipt, Error = ()> {
            let (_eloop, transport) = Http::new(&self.endpoint).unwrap();
            let web3 = Web3::new(transport);

            // 1. get the nonce
            web3.eth()
                .transaction_count(self.address.clone(), None)
                .map_err(move |err| println!("Couldn't fetch nonce. Got error: {:#?}", err))
                .and_then(move |nonce| {
                    // 2. create the transaction
                    let tx = make_tx(to, U256::from(amount), nonce);
                    // 3. sign the transaction
                    let signed_tx = self.signer.sign(tx, &1);
                    // 4. send the transaction
                    web3.send_raw_transaction_with_confirmation(
                        signed_tx.into(),
                        self.poll_frequency,
                        self.confirmations,
                    )
                    .map_err(move |err| println!("Could not submit raw transaction. Error: {:#?}", err))
                    // 5. return the receipt
                    .and_then(|receipt| Ok(receipt))
                })
        }

        #[post("/accounts/:account_id/settlement")]
        fn send_money(&self, account_id: String) -> impl Future<Item = Response<String>, Error = Response<()>> {
            ok(Response::builder().status(200).body(account_id).unwrap())
        }
    }
}