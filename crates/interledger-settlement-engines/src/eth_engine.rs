use ethereum_tx_sign::web3::{
    api::Web3,
    futures::future::{ok, result, Future},
    transports::Http,
    types::{Address, TransactionReceipt, U256},
};
use hyper::{Response, StatusCode};
use interledger_settlement::SettlementData;
use std::{marker::PhantomData, str::FromStr, time::Duration};

use super::{make_tx, Addresses, TxSigner};
use super::{EthereumAccount, EthereumStore};

#[derive(Debug, Clone, Extract)]
struct CreateAccountDetails {
    pub ethereum_address: Address,
    pub token_address: Option<Address>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum MessageType {
    Config = 0,
    PaymentChannelOpen = 1,
    PaymentChannelPay = 2,
    PaymentChannelClose = 3,
}

#[derive(Debug, Clone, Extract)]
struct ReceiveMessageDetails {
    msg_type: MessageType,
    data: Vec<u8>,
}

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
            body: ReceiveMessageDetails,
            _idempotency_key: String,
        ) -> impl Future<Item = Response<String>, Error = Response<String>> {
            let _message_type = body.msg_type;
            let _data = body.data;
            self.load_account(account_id).map_err(|err| {
                let error_msg = format!("Error loading account {:?}", err);
                error!("{}", error_msg);
                Response::builder().status(400).body(error_msg).unwrap()
            })
            .and_then(move |(_account_id, _addresses)| {
                // TODO: What functionality should exist here?
                // let (ethereum_address, token_address) = addresses;
                // match message_type {
                //     MessageType::Config => {
                //         let data = match data.len() {
                //             20 => (Address::from(&data[..]), None),
                //             40 => (Address::from(&data[..20]), Some(Address::from(&data[20..]))),
                //             _ => return Err(Response::builder().status(502).body("INVALID PAYLOAD LENGTH".to_string()).unwrap())
                //         };
                //         store.save_account_addresses(vec![account_id], vec![data]);
                //     },
                //     _ => unimplemented!()
                // }
                Ok(Response::builder()
                    .status(200)
                    .body("OK".to_string())
                    .unwrap())
            })
        }

        // TODO: it should take an account id, register it locally and then call
        // the connector's messages endpoint so that it forwards it to the other
        // engine.
        #[post("/accounts")]
        fn create_account(
            &self,
            account_id: String,
            body: CreateAccountDetails,
            _idempotency_key: Option<String>,
        ) -> impl Future<Item = Response<String>, Error = Response<String>> {
            // TODO idempotency check
            let store: S = self.store.clone();
            let data = (body.ethereum_address, body.token_address);

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
                    store.save_account_addresses(vec![account_id], vec![data])
                        .map_err(move |_err| {
                            let error_msg = format!("Error creating account: {}, {:?}", account_id, data);
                            error!("{}", error_msg);
                            // let status_code = StatusCode::from_u16(404).unwrap();
                            // let data = Bytes::from(error_msg.clone());
                            // store.save_idempotent_data(idempotency_key, status_code, data);
                            Response::builder().status(400).body(error_msg).unwrap()
                        })
                }
            })
            .and_then(move |_| {
                Ok(Response::builder()
                    .status(201)
                    .body("CREATED".to_string())
                    .unwrap())
            })

        }

        // TODO : it should get the data associated with accounts
        #[get("/accounts/:id")]
        #[content_type("application/json")]
        fn get_account(
            &self,
            account_id: String,
        ) -> impl Future<Item = Response<String>, Error = Response<String>> {
            self.load_account(account_id).map_err(|err| {
                let error_msg = format!("Error loading account {:?}", err);
                error!("{}", error_msg);
                Response::builder().status(400).body(error_msg).unwrap()
            })
            .and_then(move |(_account_id, addresses)| {
                let (ethereum_address, token_address) = addresses;
                let ret = json!({"ethereum_address" : ethereum_address, "token_address" : token_address}).to_string();
                Ok(Response::builder().status(200).body(ret).unwrap())
            })
        }


        #[post("/accounts/:account_id/settlement")]
        fn execute_settlement(
            &self,
            account_id: String,
            body: SettlementData,
            _idempotency_key: Option<String>,
        ) -> impl Future<Item = Response<String>, Error = Response<String>> {
            // TODO add idempotency check.
            let amount = U256::from(body.amount);
            let self_clone = self.clone();
            self.load_account(account_id).map_err(|err| {
                let error_msg = format!("Error loading account {:?}", err);
                error!("{}", error_msg);
                Response::builder().status(400).body(error_msg).unwrap()
            })
            .and_then(move |(_account_id, addresses)| {
                let (to, token_addr) = addresses;
                self_clone.settle_to(to, amount, token_addr).map_err(|_| {
                    let error_msg = format!("Error connecting to the blockchain.");
                    error!("{}", error_msg);
                    // maybe replace with a per-blockchain specific status code?
                    Response::builder().status(502).body(error_msg).unwrap()
                })
            })
            .and_then(move |_| {
                Ok(Response::builder()
                    .status(200)
                    .body("OK".to_string())
                    .unwrap())
            })
        }

        /// helper function that returns the addresses associated with an
        /// account from a given string account id
        fn load_account(&self, account_id: String) -> impl Future <Item = (A::AccountId, Addresses), Error = String> {
            let store = self.store.clone();
            result(A::AccountId::from_str(&account_id).map_err(move |_err| {
                // let store = store.clone();
                // let idempotency_key = idempotency_key.clone();
                // move |_err| {
                let error_msg = format!("Unable to parse account");
                error!("{}", error_msg);
                // let status_code = StatusCode::from_u16(400).unwrap();
                // let data = Bytes::from(error_msg.clone());
                // store.save_idempotent_data(idempotency_key, status_code,
                // data);
                error_msg
            }))
            .and_then(move |account_id| {
                    store
                        .load_account_addresses(vec![account_id])
                        .map_err(move |_err| {
                            let error_msg = format!("Error getting account: {}", account_id);
                            error!("{}", error_msg);
                            // let status_code = StatusCode::from_u16(404).unwrap();
                            // let data = Bytes::from(error_msg.clone());
                            // store.save_idempotent_data(idempotency_key, status_code, data);
                            error_msg
                        })
                        .and_then(move |addresses| {
                            ok((account_id, addresses[0]))
                        })
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::fixtures::BOB;
    use crate::test_helpers::{test_api, test_engine, test_store, TestAccount};
    static ALICE_PK: &str = "380eb0f3d505f087e438eca80bc4df9a7faa24f868e69fc0440261a0fc0567dc";
    static ALICE_ADDR: &str = "3cdb3d9e1b74692bb1e3bb5fc81938151ca64b02";

    #[test]
    // All tests involving ganache must be run in 1 suite so that they run serially
    fn test_execute_settlement() {
        let bob = BOB.clone();
        let store = test_store(bob.clone(), false, true, true);
        let (engine, mut ganache_pid) = test_engine(store, ALICE_PK, ALICE_ADDR, 0);
        let amount = U256::from(100000);

        let receipt = engine.settle_to(bob.address, amount, None).wait().unwrap();
        assert_eq!(receipt.status, Some(1.into()));

        let ret: Response<_> = engine
            .execute_settlement(bob.id.to_string(), SettlementData { amount: 100 }, None)
            .wait()
            .unwrap();
        assert_eq!(ret.status().as_u16(), 200);
        assert_eq!(ret.body(), "OK");

        ganache_pid.kill().unwrap();
    }

    #[test]
    fn test_create_get_account() {
        let bob: TestAccount = BOB.clone();
        let store = test_store(bob.clone(), false, false, false);
        let engine = test_api(store, ALICE_PK, ALICE_ADDR, 0);

        // Bob's ILP details have already been inserted. This endpoint gets
        // automatically called when creating the account.
        let ret: Response<_> = engine
            .create_account(
                bob.id.to_string(),
                CreateAccountDetails {
                    ethereum_address: bob.address,
                    token_address: None,
                },
                None,
            )
            .wait()
            .unwrap();
        assert_eq!(ret.status().as_u16(), 201);
        assert_eq!(ret.body(), "CREATED");

        let ret: Response<_> = engine.get_account(bob.id.to_string()).wait().unwrap();
        assert_eq!(ret.status().as_u16(), 200);
        let data = json!({"ethereum_address" : bob.address, "token_address" : null}).to_string();
        assert_eq!(ret.body(), &data);

        // todo: idempotency checks
    }

}
