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

            // FIXME: For some reason, when running asynchronously, all calls time out.
            // Are we missing something obvious?

            // 1. get the nonce
            // web3.eth()
            //     .transaction_count(self.address.clone(), None)
            //     .map_err(move |err| error!("Couldn't fetch nonce. Got error: {:#?}", err))
            //     .and_then(move |nonce| {
            //         // 2. create the transaction
            //         let tx = make_tx(to, U256::from(amount), nonce);
            //         // 3. sign the transaction
            //         let signed_tx = self.signer.sign(tx, &1);
            //         // 4. send the transaction
            //         web3.send_raw_transaction_with_confirmation(
            //             signed_tx.into(),
            //             self.poll_frequency,
            //             self.confirmations,
            //         )
            //         .map_err(move |err| error!("Could not submit raw transaction. Error: {:#?}", err))
            //         // 5. return the receipt
            //         .and_then(|receipt| Ok(receipt))
            //     })
            let nonce = web3.eth().transaction_count(self.address.clone(), None).wait().unwrap();
            let tx = make_tx(to, U256::from(amount), nonce);
            let signed_tx = self.signer.sign(tx, &1);
            let receipt = web3.send_raw_transaction_with_confirmation(signed_tx.into(), self.poll_frequency, self.confirmations).wait().unwrap();
            ok(receipt)
        }

        #[post("/accounts/:account_id/settlement")]
        fn send_money(&self, account_id: String) -> impl Future<Item = Response<String>, Error = Response<()>> {
            // self.settle_to(self.address, U256::from(300));
            ok(Response::builder().status(200).body(account_id).unwrap())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    use std::str::FromStr;
    use std::thread::sleep;
    // NOTE: Tests require ganache-cli instance to be in $PATH.

    static ALICE_PK: &str = "380eb0f3d505f087e438eca80bc4df9a7faa24f868e69fc0440261a0fc0567dc";
    static ALICE_ADDR: &str = "3cdb3d9e1b74692bb1e3bb5fc81938151ca64b02";
    static BOB_ADDR: &str = "2fcd07047c209c46a767f8338cb0b14955826826";

    #[test]
    fn test_send_tx() {
        let (engine, mut ganache_pid) = test_engine(ALICE_PK, ALICE_ADDR, 0);
        let amount = U256::from(100000);

        let id = "1".to_string();
        let receipt = engine
            .settle_to(BOB_ADDR.parse().unwrap(), amount)
            .wait()
            .unwrap();
        assert_eq!(receipt.status, Some(1.into()));
        ganache_pid.kill().unwrap();
    }

    // Helper to create a new engine and spin a new ganache instance.
    fn test_engine<S>(
        key: S,
        addr: &str,
        confs: usize,
    ) -> (EthereumSettlementEngine<S>, std::process::Child)
    where
        S: TxSigner + Send + Sync + 'static,
    {
        let mut ganache = Command::new("ganache-cli");
        let ganache = ganache
            .stdout(std::process::Stdio::null())
            .arg("-m")
            .arg("abstract vacuum mammal awkward pudding scene penalty purchase dinner depart evoke puzzle");
        let ganache_pid = ganache.spawn().expect("couldnt start ganache-cli");
        // wait a couple of seconds for ganache to boot up
        sleep(Duration::from_secs(3));
        let chain_id = 1;
        let poll_frequency = Duration::from_secs(1);
        let engine = EthereumSettlementEngine::new(
            "http://localhost:8545".to_string(),
            key,
            Address::from_str(addr).unwrap(),
            chain_id,
            confs,
            poll_frequency,
        );

        (engine, ganache_pid)
    }
}
