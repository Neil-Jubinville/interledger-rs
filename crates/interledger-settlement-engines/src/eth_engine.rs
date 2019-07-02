use ethereum_tx_sign::web3::{
    api::Web3,
    futures::future::{ok, result, Future},
    transports::Http,
    types::{Address, TransactionReceipt, U256},
};
use hyper::{Response, StatusCode};
use interledger_settlement::SettlementData;
use std::{marker::PhantomData, str::FromStr, time::Duration};

use super::{make_tx, TxSigner};
use super::{EthereumAccount, EthereumStore};
use interledger_ildcp::IldcpAccount;

pub struct EthereumSettlementEngine<S, Si, A> {
    store: S,
    signer: Si,
    account_type: PhantomData<A>,

    // Configuration data
    pub endpoint: String,
    pub address: Address,
    pub chain_id: u8,
    pub confirmations: usize,
    pub poll_frequency: Duration,
}

impl_web! {
    impl<S, Si, A> EthereumSettlementEngine<S, Si, A>
    where
        S:  EthereumStore<Account = A> + Clone + Send + Sync + 'static,
        Si: TxSigner + Send + Sync + 'static,
        A:  EthereumAccount + IldcpAccount + Send + Sync + 'static,
    {
        pub fn new(
            endpoint: String,
            store: S,
            signer: Si,
            address: Address,
            chain_id: u8,
            confirmations: usize,
            poll_frequency: Duration,
        ) -> Self {
            EthereumSettlementEngine {
                endpoint,
                store,
                signer,
                address,
                chain_id,
                confirmations,
                poll_frequency,
                account_type: PhantomData,
            }
        }

        // this should return a future that will be chained with the message call to
        // the connector amount in wei
        pub fn settle_to(
            &self,
            to: Address,
            amount: U256,
            token_address: Option<Address>
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
            let tx = make_tx(to, U256::from(amount), nonce, token_address);
            let signed_tx = self.signer.sign(tx, &1);
            let receipt = web3.send_raw_transaction_with_confirmation(signed_tx.into(), self.poll_frequency, self.confirmations).wait().unwrap();
            ok(receipt)
        }

        #[post("/accounts/:account_id/settlement")]
        fn execute_settlement(&self, account_id: String, body: SettlementData, _idempotency_key: String) -> impl Future<Item = Response<String>, Error = Response<String>> {
            // TODO add idempotency check.
            let _amount = U256::from(body.amount);
            let store = self.store.clone();
            result(A::AccountId::from_str(&account_id)
            .map_err(move |_err| {
                // let store = store.clone();
                // let idempotency_key = idempotency_key.clone();
                // move |_err| {
                let error_msg = format!("Unable to parse account");
                error!("{}", error_msg);
                // let status_code = StatusCode::from_u16(400).unwrap();
                // let data = Bytes::from(error_msg.clone());
                // store.save_idempotent_data(idempotency_key, status_code, data);
                Response::builder().status(400).body(error_msg).unwrap()
            })).and_then({
                let store = store.clone();
                move |account_id| {
                    store.load_account_addresses(vec![account_id])
                    .map_err(move |_err| {
                        let error_msg = format!("Error getting account: {}", account_id);
                        error!("{}", error_msg);
                        // let status_code = StatusCode::from_u16(404).unwrap();
                        // let data = Bytes::from(error_msg.clone());
                        // store.save_idempotent_data(idempotency_key, status_code, data);
                        Response::builder().status(400).body(error_msg).unwrap()
                    })
                    .and_then(move |addresses| {
                        // FIXME FIGURE OUT WHY WE GET LIFETIME ERRORS HERE.
                        let (_to, _token_addr) = &addresses[0];
                        // self.settle_to(self.address, U256::from(amount), token_addr).map_err(|_| {
                            // let error_msg = format!("Error connecting to the blockchain.");
                            // error!("{}", error_msg);
                            // Response::builder().status(502).body(error_msg).unwrap()
                        // })
                        ok(1)
                    })
                }})
            .and_then(move |_| Ok(
                Response::builder().status(200).body("Success!".to_string()).unwrap()
            ))
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
    ) -> (EthereumSettlementEngine<S, Si, A>, std::process::Child)
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
