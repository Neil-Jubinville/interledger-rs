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

#[derive(Debug, Clone)]
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
        S: EthereumStore<Account = A> + Clone + Send + Sync + 'static,
        Si: TxSigner + Clone + Send + Sync + 'static,
        A: EthereumAccount + Send + Sync + 'static,
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

        /// Submits a transaction to `to` the Ethereum blockchain for `amount`.
        /// If called with `token_address`, it makes an ERC20 transaction instead.
        fn settle_to(
            // must live longer than execute
            &self,
            to: Address,
            amount: U256,
            token_address: Option<Address>,
        ) -> impl Future<Item = TransactionReceipt, Error = ()> {
            // FIXME: We initialize inside the function and not outside due to
            // invalid pipe errors (for some reason...)
            let (_eloop, transport) = Http::new(&self.endpoint).unwrap();
            let web3 = Web3::new(transport);

            // FIXME: For some reason, when running asynchronously, all calls time out.
            // 1. get the nonce
            // let self_clone = self.clone();
            // web3.eth()
            //     .transaction_count(self.address.clone(), None)
            //     .map_err(move |err| error!("Couldn't fetch nonce. Got error: {:#?}", err))
            //     .and_then(move |nonce| {
            //         // 2. create the transaction
            //         let tx = make_tx(to, U256::from(amount), nonce, token_address);
            //         // 3. sign the transaction
            //         let signed_tx = self_clone.signer.sign(tx, &1);
            //         // 4. send the transaction

            //         web3.send_raw_transaction_with_confirmation(
            //             signed_tx.into(),
            //             self_clone.poll_frequency,
            //             self_clone.confirmations,
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

        // TODO: Receive message is going to be utilized for L2 protocols and
        // for configuring the engine. We can make the body class as:
        // type : data related to that. depending on type it should have
        // different encoding, via some enum. for now we cna im plement a config
        // message, then we can add a paychann message.
        #[post("/accounts/:account_id/messages")]
        fn receive_message(
            &self,
            account_id: String,
            body: Vec<u8>,
            _idempotency_key: String,
        ) -> impl Future<Item = Response<String>, Error = Response<String>> {
            unimplemented!()
        }

        // TODO: it should take an account id, register it locally and then call
        // the connector's messages endpoint so that it forwards it to the other
        // engine.
        #[post("/accounts")]
        fn create_account(
            &self,
            account_id: String,
            body: Vec<u8>,
            _idempotency_key: String,
        ) -> impl Future<Item = Response<String>, Error = Response<String>> {
            unimplemented!()
        }

        // TODO : it should get the data associated with accounts
        #[get("/accounts")]
        fn get_account(
            &self,
            account_id: String,
            body: Vec<u8>,
            _idempotency_key: String,
        ) -> impl Future<Item = Response<String>, Error = Response<String>> {
            unimplemented!()
        }

        #[post("/accounts/:account_id/settlement")]
        fn execute_settlement(
            &self,
            account_id: String,
            body: SettlementData,
            _idempotency_key: String,
        ) -> impl Future<Item = Response<String>, Error = Response<String>> {
            // TODO add idempotency check.
            let amount = U256::from(body.amount);
            let self_clone = self.clone();
            let store = self.store.clone();
            result(A::AccountId::from_str(&account_id).map_err(move |_err| {
                // let store = store.clone();
                // let idempotency_key = idempotency_key.clone();
                // move |_err| {
                let error_msg = format!("Unable to parse account");
                error!("{}", error_msg);
                // let status_code = StatusCode::from_u16(400).unwrap();
                // let data = Bytes::from(error_msg.clone());
                // store.save_idempotent_data(idempotency_key, status_code, data);
                Response::builder().status(400).body(error_msg).unwrap()
            }))
            .and_then({
                move |account_id| {
                    store
                        .load_account_addresses(vec![account_id])
                        .map_err(move |_err| {
                            let error_msg = format!("Error getting account: {}", account_id);
                            error!("{}", error_msg);
                            // let status_code = StatusCode::from_u16(404).unwrap();
                            // let data = Bytes::from(error_msg.clone());
                            // store.save_idempotent_data(idempotency_key, status_code, data);
                            Response::builder().status(400).body(error_msg).unwrap()
                        })
                }
            })
            .and_then({
                move |addresses| {
                    // tood handle None
                let (to, token_addr) = addresses[0].unwrap();
                // FIXME: Figure out why we get lifetime errors here.
                // settle_to MUST outlive execute_settlement.
                self_clone.settle_to(to, amount, token_addr).map_err(|_| {
                    let error_msg = format!("Error connecting to the blockchain.");
                    error!("{}", error_msg);
                    // maybe replace with a per-blockchain specific status code?
                    Response::builder().status(502).body(error_msg).unwrap()
                })
            }})
            .and_then(move |_| {
                Ok(Response::builder()
                    .status(200)
                    .body("Success!".to_string())
                    .unwrap())
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{test_engine, test_store};

    static ALICE_PK: &str = "380eb0f3d505f087e438eca80bc4df9a7faa24f868e69fc0440261a0fc0567dc";
    static ALICE_ADDR: &str = "3cdb3d9e1b74692bb1e3bb5fc81938151ca64b02";
    static BOB_ADDR: &str = "2fcd07047c209c46a767f8338cb0b14955826826";

    #[test]
    fn test_send_tx() {
        let store = test_store(false, true);
        let (engine, mut ganache_pid) = test_engine(store, ALICE_PK, ALICE_ADDR, 0);
        let amount = U256::from(100000);

        let receipt = engine
            .settle_to(BOB_ADDR.parse().unwrap(), amount, None)
            .wait()
            .unwrap();
        assert_eq!(receipt.status, Some(1.into()));
        ganache_pid.kill().unwrap();
    }

}
