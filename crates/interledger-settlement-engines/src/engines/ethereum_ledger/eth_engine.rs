use super::types::{Addresses, EthereumAccount, EthereumLedgerTxSigner, EthereumStore};
use super::utils::make_tx;

use bytes::Bytes;
use ethereum_tx_sign::web3::{
    api::Web3,
    futures::future::{err, ok, result, Either, Future},
    transports::Http,
    types::{Address, U256},
};
use hyper::{Response, StatusCode};
use interledger_settlement::{IdempotentStore, SettlementData};
use reqwest::r#async::Client;
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use std::{marker::PhantomData, str::FromStr, time::Duration};
use tokio_executor::spawn;
use url::Url;
use uuid::Uuid;

use crate::SettlementEngine;

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
pub struct EthereumLedgerSettlementEngine<S, Si, A> {
    store: S,
    signer: Si,
    account_type: PhantomData<A>,

    // Configuration data
    pub endpoint: String,
    pub address: Address,
    pub chain_id: u8,
    pub confirmations: usize,
    pub poll_frequency: Duration,
    pub connector_url: Url,
}

impl<S, Si, A> EthereumLedgerSettlementEngine<S, Si, A>
where
    S: EthereumStore<Account = A> + IdempotentStore + Clone + Send + Sync + 'static,
    Si: EthereumLedgerTxSigner + Clone + Send + Sync + 'static,
    A: EthereumAccount + Send + Sync + 'static,
{
    pub fn new(
        endpoint: String,
        store: S,
        signer: Si,
        chain_id: u8,
        confirmations: usize,
        poll_frequency: Duration,
        connector_url: Url,
    ) -> Self {
        let address = signer.address();
        EthereumLedgerSettlementEngine {
            endpoint,
            store,
            signer,
            address,
            chain_id,
            confirmations,
            poll_frequency,
            connector_url,
            account_type: PhantomData,
        }
    }

    /// Submits a transaction to `to` the Ethereum blockchain for `amount`.
    /// If called with `token_address`, it makes an ERC20 transaction instead.
    pub fn settle_to(
        &self,
        to: Address,
        to_account_id: String,
        amount: U256,
        token_address: Option<Address>,
    ) -> Box<dyn Future<Item = (), Error = ()> + Send> {
        let (_eloop, transport) = Http::new(&self.endpoint).unwrap();
        let web3 = Web3::new(transport);

        // TODO: Convert to and_then syntax once
        // https://github.com/tomusdrw/rust-web3/issues/227 is resolved

        // get the account's nonce
        let nonce = web3
            .eth()
            .transaction_count(self.address, None)
            .wait()
            .unwrap();

        // create the signed transaction
        let tx = make_tx(to, amount, nonce, token_address);
        let signed_tx = self.signer.sign(tx, self.chain_id);

        // submit it and wait for sufficient confirmations
        let tx_receipt = web3
            .send_raw_transaction_with_confirmation(
                signed_tx.into(),
                self.poll_frequency,
                self.confirmations,
            )
            .wait()
            .unwrap();
        let tx_receipt_clone = tx_receipt.clone();

        let mut url = self.connector_url.clone();
        url.path_segments_mut()
            .expect("Invalid connector URL")
            .push("accounts")
            .push(&to_account_id)
            .push("settlement");

        // notify the accounting system about the completion of the settlement
        let client = Client::new();
        let idempotency_uuid = Uuid::new_v4().to_hyphenated().to_string();
        Box::new(client.post(url)
            .header("Content-Type", "application/octet-stream")
            .header("Idempotency-Key", idempotency_uuid)
            .body(amount.to_string())
            .send()
            .map_err(move |err| error!("Error notifying accounting system about transaction {:?}: {:?}", tx_receipt_clone, err))
            .and_then(move |response| {
                if response.status().is_success() {
                    trace!("Successfully notified accounting system about the settlement of: {:?}", tx_receipt);
                    Ok(())
                } else {
                    error!("Error notifying accounting system about transaction {:?}. It responded with HTTP code: {}", tx_receipt, response.status());
                    Err(())
                }
            })
        )
    }

    #[allow(unused)]
    fn get_account(
        &self,
        account_id: String,
    ) -> impl Future<Item = Response<String>, Error = Response<String>> {
        self.load_account(account_id)
            .map_err(|err| {
                let error_msg = format!("Error loading account {:?}", err);
                error!("{}", error_msg);
                Response::builder().status(400).body(error_msg).unwrap()
            })
            .and_then(move |(_account_id, addresses)| {
                let j = serde_json::to_string(&addresses).unwrap();
                Ok(Response::builder().status(200).body(j).unwrap())
            })
    }

    /// helper function that returns the addresses associated with an
    /// account from a given string account id
    fn load_account(
        &self,
        account_id: String,
    ) -> impl Future<Item = (A::AccountId, Addresses), Error = String> {
        let store = self.store.clone();
        result(A::AccountId::from_str(&account_id).map_err(move |_err| {
            let error_msg = "Unable to parse account".to_string();
            error!("{}", error_msg);
            error_msg
        }))
        .and_then(move |account_id| {
            store
                .load_account_addresses(vec![account_id])
                .map_err(move |_err| {
                    let error_msg = format!("Error getting account: {}", account_id);
                    error!("{}", error_msg);
                    error_msg
                })
                .and_then(move |addresses| ok((account_id, addresses[0].clone())))
        })
    }

    fn check_idempotency(
        &self,
        idempotency_key: Option<String>,
        input_hash: [u8; 32],
    ) -> impl Future<Item = Option<(StatusCode, Bytes)>, Error = (StatusCode, String)> {
        self.store
            .load_idempotent_data(idempotency_key.clone())
            .map_err(move |err| {
                let err = format!("Couldn't connect to store {:?}", err);
                error!("{}", err);
                (StatusCode::from_u16(500).unwrap(), err)
            })
            .and_then(move |ret: Option<(StatusCode, Bytes, [u8; 32])>| {
                if let Some(d) = ret {
                    if d.2 != input_hash {
                        // Stripe CONFLICT status code
                        return Err((
                            StatusCode::from_u16(409).unwrap(),
                            "Provided idempotency key is tied to other input".to_string(),
                        ));
                    }
                    if d.0.is_success() {
                        return Ok(Some((d.0, d.1)));
                    } else {
                        return Err((d.0, String::from_utf8_lossy(&d.1).to_string()));
                    }
                }
                Ok(None)
            })
    }
}

impl<S, Si, A> SettlementEngine for EthereumLedgerSettlementEngine<S, Si, A>
where
    S: EthereumStore<Account = A> + IdempotentStore + Clone + Send + Sync + 'static,
    Si: EthereumLedgerTxSigner + Clone + Send + Sync + 'static,
    A: EthereumAccount + Send + Sync + 'static,
{
    // TODO: Receive message is going to be utilized for L2 protocols and
    // for configuring the engine. We can make the body class as:
    // type : data related to that. depending on type it should have
    // different encoding, via some enum. for now we cna im plement a config
    // message, then we can add a paychann message.
    fn receive_message(
        &self,
        account_id: String,
        _body: Vec<u8>,
        _idempotency_key: Option<String>,
    ) -> Box<dyn Future<Item = Response<String>, Error = Response<String>> + Send> {
        // let _message_type = body.msg_type; // todo: maybe add some parsing logic
        // let _data = body.data;
        Box::new(
            self.load_account(account_id)
                .map_err(|err| {
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
                }),
        )
    }

    fn create_account(
        &self,
        account_id: String,
        body: Vec<u8>,
        idempotency_key: Option<String>,
    ) -> Box<dyn Future<Item = Response<String>, Error = Response<String>> + Send> {
        let self_clone = self.clone();
        let store: S = self.store.clone();
        let store_clone = self.store.clone();
        let store_clone2 = self.store.clone();
        let idempotency_key_clone = idempotency_key.clone();

        let input = format!("{}{:?}", account_id, body);
        let input_hash = get_hash_of(input.as_ref());

        Box::new(
            self_clone
                .check_idempotency(idempotency_key.clone(), input_hash)
                .map_err(|res| Response::builder().status(res.0).body(res.1).unwrap())
                .and_then(move |ret: Option<(StatusCode, Bytes)>| {
                    if let Some(d) = ret {
                        return Either::A(ok(Response::builder()
                            .status(d.0)
                            .body(String::from_utf8_lossy(&d.1).to_string())
                            .unwrap()));
                    }
                    Either::B(
                        result(serde_json::from_slice(&body).map_err(move |_err| {
                            let error_msg = "Unable to parse message body".to_string();
                            error!("{}", error_msg);
                            Response::builder().status(400).body(error_msg).unwrap()
                        }))
                        .and_then(move |addresses: Addresses| {
                            result(A::AccountId::from_str(&account_id).map_err({
                                let store = store.clone();
                                let idempotency_key = idempotency_key.clone();
                                move |_err| {
                                    let error_msg = "Unable to parse account".to_string();
                                    error!("{}", error_msg);
                                    let status_code = StatusCode::from_u16(400).unwrap();
                                    let data = Bytes::from(error_msg.clone());
                                    spawn(store.save_idempotent_data(
                                        idempotency_key,
                                        input_hash,
                                        status_code,
                                        data,
                                    ));
                                    Response::builder()
                                        .status(status_code)
                                        .body(error_msg)
                                        .unwrap()
                                }
                            }))
                            .and_then({
                                move |account_id| {
                                    store
                                        .save_account_addresses(vec![account_id], vec![addresses])
                                        .map_err(move |_err| {
                                            let error_msg =
                                                format!("Error creating account: {}", account_id);
                                            error!("{}", error_msg);
                                            let status_code = StatusCode::from_u16(400).unwrap();
                                            let data = Bytes::from(error_msg.clone());
                                            spawn(store_clone.save_idempotent_data(
                                                idempotency_key,
                                                input_hash,
                                                status_code,
                                                data,
                                            ));
                                            Response::builder().status(400).body(error_msg).unwrap()
                                        })
                                }
                            })
                            .and_then(move |_| {
                                spawn(store_clone2.save_idempotent_data(
                                    idempotency_key_clone,
                                    input_hash,
                                    StatusCode::from_u16(201).unwrap(),
                                    Bytes::from("CREATED"),
                                ));
                                Ok(Response::builder()
                                    .status(201)
                                    .body("CREATED".to_string())
                                    .unwrap())
                            })
                        }),
                    )
                }),
        )
    }

    fn send_money(
        &self,
        account_id: String,
        body: SettlementData,
        idempotency_key: Option<String>,
    ) -> Box<dyn Future<Item = Response<String>, Error = Response<String>> + Send> {
        let amount = U256::from(body.amount);
        let self_clone = self.clone();
        let store = self.store.clone();
        let store_clone = store.clone();
        let store_clone2 = store.clone();
        let idempotency_key_clone = idempotency_key.clone();
        let idempotency_key_clone2 = idempotency_key.clone();
        let account_id_clone = account_id.clone();

        let input = format!("{}{:?}", account_id, body);
        let input_hash = get_hash_of(input.as_ref());

        Box::new(
            self.check_idempotency(idempotency_key.clone(), input_hash)
                .map_err(|res| Response::builder().status(res.0).body(res.1).unwrap())
                .and_then(move |ret: Option<(StatusCode, Bytes)>| {
                    if let Some(d) = ret {
                        return Either::A(ok(Response::builder()
                            .status(d.0)
                            .body(String::from_utf8_lossy(&d.1).to_string())
                            .unwrap()));
                    }
                    Either::B(
                        self_clone
                            .load_account(account_id)
                            .map_err(move |err| {
                                let error_msg = format!("Error loading account {:?}", err);
                                error!("{}", error_msg);
                                spawn(store.save_idempotent_data(
                                    idempotency_key,
                                    input_hash,
                                    StatusCode::from_u16(400).unwrap(),
                                    Bytes::from(error_msg.clone()),
                                ));
                                Response::builder().status(400).body(error_msg).unwrap()
                            })
                            .and_then(move |(_account_id, addresses)| {
                                self_clone
                                    .settle_to(
                                        addresses.own_address,
                                        account_id_clone,
                                        amount,
                                        addresses.token_address,
                                    )
                                    .map_err(move |_| {
                                        let error_msg =
                                            "Error connecting to the blockchain.".to_string();
                                        error!("{}", error_msg);
                                        // maybe replace with a per-blockchain specific status code?
                                        spawn(store_clone.save_idempotent_data(
                                            idempotency_key_clone,
                                            input_hash,
                                            StatusCode::from_u16(502).unwrap(),
                                            Bytes::from(error_msg.clone()),
                                        ));
                                        Response::builder().status(502).body(error_msg).unwrap()
                                    })
                            })
                            .and_then(move |_| {
                                spawn(store_clone2.save_idempotent_data(
                                    idempotency_key_clone2,
                                    input_hash,
                                    StatusCode::from_u16(200).unwrap(),
                                    Bytes::from("OK".to_string()),
                                ));
                                Ok(Response::builder()
                                    .status(200)
                                    .body("OK".to_string())
                                    .unwrap())
                            }),
                    )
                }),
        )
    }
}

fn get_hash_of(preimage: &[u8]) -> [u8; 32] {
    let mut hash = [0; 32];
    hash.copy_from_slice(digest(&SHA256, preimage).as_ref());
    hash
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{ALICE, BOB, SETTLEMENT_API};
    use super::super::test_helpers::{block_on, test_api, test_engine, test_store, TestAccount};
    use super::*;
    use mockito;

    static IDEMPOTENCY: &str = "AJKJNUjM0oyiAN46";
    static IDEMPOTENCY_FAIL: &str = "AAAAAAAAAAAAAAAA";

    lazy_static! {
        pub static ref ALICE_PK: String =
            String::from("380eb0f3d505f087e438eca80bc4df9a7faa24f868e69fc0440261a0fc0567dc");
    }

    #[test]
    // All tests involving ganache must be run in 1 suite so that they run serially
    fn test_send_money() {
        let m = mockito::mock("POST", SETTLEMENT_API.clone())
            .match_header("content-type", "application/octet-stream")
            .with_status(200)
            .with_body("OK".to_string())
            .expect(1) // only 1 request is made to the connector (idempotency works properly)
            .create();
        let connector_url = mockito::server_url();
        let bob = BOB.clone();
        let store = test_store(bob.clone(), false, true, true);
        let (engine, mut ganache_pid) =
            test_engine(store.clone(), ALICE_PK.clone(), 0, connector_url.into());

        let ret: Response<_> = block_on(engine.send_money(
            bob.id.to_string(),
            SettlementData { amount: 100 },
            Some(IDEMPOTENCY.to_string()),
        ))
        .unwrap();
        assert_eq!(ret.status().as_u16(), 200);
        assert_eq!(ret.body(), "OK");

        let ret: Response<_> = block_on(engine.send_money(
            bob.id.to_string(),
            SettlementData { amount: 100 },
            Some(IDEMPOTENCY.to_string()),
        ))
        .unwrap();
        assert_eq!(ret.status().as_u16(), 200);
        assert_eq!(ret.body(), "OK");

        // fails with different id and same data
        let ret: Response<_> = block_on(engine.send_money(
            "42".to_string(),
            SettlementData { amount: 100 },
            Some(IDEMPOTENCY.to_string()),
        ))
        .unwrap_err();
        assert_eq!(ret.status().as_u16(), 409);
        assert_eq!(
            ret.body(),
            "Provided idempotency key is tied to other input"
        );

        // fails with same id and different data
        let ret: Response<_> = block_on(engine.send_money(
            bob.id.to_string(),
            SettlementData { amount: 42 },
            Some(IDEMPOTENCY.to_string()),
        ))
        .unwrap_err();
        assert_eq!(ret.status().as_u16(), 409);
        assert_eq!(
            ret.body(),
            "Provided idempotency key is tied to other input"
        );

        // fails with different id and different data
        let ret: Response<_> = block_on(engine.send_money(
            "42".to_string(),
            SettlementData { amount: 42 },
            Some(IDEMPOTENCY.to_string()),
        ))
        .unwrap_err();
        assert_eq!(ret.status().as_u16(), 409);
        assert_eq!(
            ret.body(),
            "Provided idempotency key is tied to other input"
        );

        let s = store.clone();
        let cache = s.cache.read();
        let cached_data = cache.get(&IDEMPOTENCY.to_string()).unwrap();

        let cache_hits = s.cache_hits.read();
        assert_eq!(*cache_hits, 4);
        assert_eq!(cached_data.0, 200);
        assert_eq!(cached_data.1, "OK".to_string());

        m.assert();
        ganache_pid.kill().unwrap();
    }

    #[test]
    fn test_create_get_account() {
        let bob: TestAccount = BOB.clone();
        let store = test_store(bob.clone(), false, false, false);
        let engine = test_api(store.clone(), ALICE_PK.clone(), 0);

        // fails on invalid input data
        let create_account_details = vec![1];
        let ret: Response<_> = block_on(engine.create_account(
            bob.id.to_string(),
            create_account_details.clone().into(),
            Some(IDEMPOTENCY_FAIL.to_string()),
        ))
        .unwrap_err();
        assert_eq!(ret.status().as_u16(), 400);
        assert_eq!(ret.body(), "Unable to parse message body");

        let create_account_details = json!({
            "own_address": bob.address,
            "token_address": null,
        })
        .to_string();

        let ret: Response<_> = block_on(engine.create_account(
            bob.id.to_string(),
            create_account_details.clone().into(),
            Some(IDEMPOTENCY.to_string()),
        ))
        .unwrap();
        assert_eq!(ret.status().as_u16(), 201);
        assert_eq!(ret.body(), "CREATED");

        let ret: Response<_> = engine.get_account(bob.id.to_string()).wait().unwrap();
        assert_eq!(ret.status().as_u16(), 200);
        assert_eq!(ret.body(), &create_account_details);

        // check that it's idempotent
        let ret: Response<_> = block_on(engine.create_account(
            bob.id.to_string(),
            create_account_details.clone().into(),
            Some(IDEMPOTENCY.to_string()),
        ))
        .unwrap();
        assert_eq!(ret.status().as_u16(), 201);
        assert_eq!(ret.body(), "CREATED");

        // fails with different id and same data
        let ret: Response<_> = block_on(engine.create_account(
            "42".to_string(),
            create_account_details.clone().into(),
            Some(IDEMPOTENCY.to_string()),
        ))
        .unwrap_err();
        assert_eq!(ret.status().as_u16(), 409);
        assert_eq!(
            ret.body(),
            "Provided idempotency key is tied to other input"
        );

        // // fails with same id and different data
        let create_account_details = json!({
            "own_address": ALICE.address,
            "token_address": null,
        })
        .to_string();
        let ret: Response<_> = block_on(engine.create_account(
            bob.id.to_string(),
            create_account_details.clone().into(),
            Some(IDEMPOTENCY.to_string()),
        ))
        .unwrap_err();
        assert_eq!(ret.status().as_u16(), 409);
        assert_eq!(
            ret.body(),
            "Provided idempotency key is tied to other input"
        );

        // fails with different id and different data
        let ret: Response<_> = block_on(engine.create_account(
            "42".to_string(),
            create_account_details.into(),
            Some(IDEMPOTENCY.to_string()),
        ))
        .unwrap_err();
        assert_eq!(ret.status().as_u16(), 409);
        assert_eq!(
            ret.body(),
            "Provided idempotency key is tied to other input"
        );

        let s = store.clone();
        let cache = s.cache.read();
        let cached_data = cache.get(&IDEMPOTENCY.to_string()).unwrap();

        let cache_hits = s.cache_hits.read();
        assert_eq!(*cache_hits, 4);
        assert_eq!(cached_data.0, 201);
        assert_eq!(cached_data.1, "CREATED".to_string());
    }
}
