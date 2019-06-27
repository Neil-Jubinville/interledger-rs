use super::{SettlementAccount, SettlementStore};
use bytes::Bytes;
use futures::{
    future::{ok, result, Either},
    Future,
};
use hyper::{Response, StatusCode};
use interledger_ildcp::IldcpAccount;
use interledger_packet::PrepareBuilder;
use interledger_service::{AccountStore, OutgoingRequest, OutgoingService};
use interledger_service_util::{Convert, ConvertDetails};
use std::{
    marker::PhantomData,
    str::{self, FromStr},
    time::{Duration, SystemTime},
};
use tower_web::{net::ConnectionStream, ServiceBuilder};

static PEER_PROTOCOL_CONDITION: [u8; 32] = [
    102, 104, 122, 173, 248, 98, 189, 119, 108, 143, 193, 139, 142, 159, 142, 32, 8, 151, 20, 133,
    110, 226, 51, 179, 144, 42, 89, 29, 13, 95, 41, 37,
];

#[derive(Debug, Clone, Extract)]
struct SettlementData {
    amount: u64,
}

pub struct SettlementApi<S, O, A> {
    outgoing_handler: O,
    store: S,
    account_type: PhantomData<A>,
}

impl_web! {
    impl<S, O, A> SettlementApi<S, O, A>
    where
        S: SettlementStore<Account = A> + AccountStore<Account = A> + Clone + Send + Sync + 'static,
        O: OutgoingService<A> + Clone + Send + Sync + 'static,
        A: SettlementAccount + IldcpAccount + Send + Sync + 'static,
    {
        pub fn new(store: S, outgoing_handler: O) -> Self {
            SettlementApi {
                store,
                outgoing_handler,
                account_type: PhantomData,
            }
        }

        fn check_idempotency(
            &self,
            idempotency_key: String
        ) -> impl Future<Item = Option<(StatusCode, Bytes)>, Error = (StatusCode, Bytes)> {
            self.store.load_idempotent_data(idempotency_key.clone())
            .map_err(move |err| {
                let err = format!("Couldn't connect to store {:?}", err);
                error!("{}", err);
                (StatusCode::from_u16(500).unwrap(), Bytes::from(err))
            }).and_then(move |ret: Option<(StatusCode, Bytes)>| {
                    if let Some(d) = ret {
                        if d.0.is_success() {
                            return Ok(Some((d.0, d.1)))
                        } else {
                            return Err((d.0, d.1))
                        }
                    }
                    Ok(None)
                }
            )
        }

        #[post("/accounts/:account_id/settlement")]
        fn receive_settlement(&self, account_id: String, body: SettlementData, idempotency_key: String) -> impl Future<Item = Response<Bytes>, Error = Response<Bytes>> {
            let amount = body.amount;
            let store = self.store.clone();

            self.check_idempotency(idempotency_key.clone()).map_err(|res| {
                Response::builder().status(res.0).body(res.1).unwrap()
            })
            .and_then(move |ret: Option<(StatusCode, Bytes)>| {
                if let Some(d) = ret {
                    return Either::A(ok(Response::builder().status(d.0).body(d.1).unwrap()));
                }
                Either::B(
                    result(A::AccountId::from_str(&account_id)
                    .map_err({
                        let store = store.clone();
                        let idempotency_key = idempotency_key.clone();
                        move |_err| {
                        let error_msg = format!("Unable to parse account id: {}", account_id);
                        error!("{}", error_msg);
                        let status_code = StatusCode::from_u16(400).unwrap();
                        let data = Bytes::from(error_msg);
                        store.save_idempotent_data(idempotency_key, status_code, data.clone());
                        Response::builder().status(400).body(data).unwrap()
                    }}))
                    .and_then({
                        let store = store.clone();
                        let idempotency_key = idempotency_key.clone();
                        move |account_id| {
                        store.get_accounts(vec![account_id])
                        .map_err({
                            let store = store.clone();
                            let idempotency_key = idempotency_key.clone();
                            move |_err| {
                            let error_msg = format!("Error getting account: {}", account_id);
                            error!("{}", error_msg);

                            let status_code = StatusCode::from_u16(404).unwrap();
                            let data = Bytes::from(error_msg);
                            store.save_idempotent_data(idempotency_key.clone(), status_code, data.clone());
                            Response::builder().status(404).body(data).unwrap()
                        }})
                    }})
                    .and_then({
                        let store = store.clone();
                        let idempotency_key = idempotency_key.clone();
                        move |accounts| {
                        let account = &accounts[0];
                        if let Some(settlement_engine) = account.settlement_engine_details() {
                            Ok((account.clone(), settlement_engine))
                        } else {
                            let error_msg = format!("Account {} does not have settlement engine details configured. Cannot handle incoming settlement", account.id());
                            error!("{}", error_msg);
                            let data = Bytes::from(error_msg);
                            store.save_idempotent_data(idempotency_key, StatusCode::from_u16(404).unwrap(), data.clone());
                            Err(Response::builder().status(404).body(data).unwrap())
                        }
                    }})
                    .and_then({
                        let store = store.clone();
                        let idempotency_key = idempotency_key.clone();
                        move |(account, settlement_engine)| {
                        let account_id = account.id();
                        let amount = amount.normalize_scale(ConvertDetails {
                            from: account.asset_scale(),
                            to: settlement_engine.asset_scale
                        });
                        store.update_balance_for_incoming_settlement(account_id, amount, idempotency_key)
                            .map_err(move |_| {
                                let err = format!("Error updating balance of account: {} for incoming settlement of amount: {}", account_id, amount);
                                error!("{}", err);
                                Response::builder().status(500).body(Bytes::from(err)).unwrap()
                        })
                    }}).and_then({
                        let store = store.clone();
                        let idempotency_key = idempotency_key.clone();
                        move |_| {
                        let ret = Bytes::from("Success");
                        store.save_idempotent_data(idempotency_key, StatusCode::OK, ret.clone())
                        .map_err(move |err| {
                            let err = format!("Couldn't connect to store {:?}", err);
                            error!("{}", err);
                            Response::builder().status(500).body(Bytes::from(err)).unwrap()
                        })
                        .and_then(|_| Ok(Response::builder().status(StatusCode::OK).body(ret).unwrap()))
                    }}))
                })
        }

        // Gets called by our settlement engine, forwards the request outwards
        // until it reaches the peer's settlement engine. Extract is not
        // implemented for Bytes unfortunately.
        #[post("/accounts/:account_id/messages")]
        fn send_outgoing_message(&self, account_id: String, body: Vec<u8>, idempotency_key: String)-> impl Future<Item = Response<Bytes>, Error = Response<Bytes>> {
            let store = self.store.clone();
            let mut outgoing_handler = self.outgoing_handler.clone();

            self.check_idempotency(idempotency_key.clone()).map_err(|res| {
                Response::builder().status(res.0).body(res.1).unwrap()
            })
            .and_then(move |ret: Option<(StatusCode, Bytes)>| {
                if let Some(d) = ret {
                    return Either::A(ok(Response::builder().status(d.0).body(d.1).unwrap()));
                }
                Either::B(
                result(A::AccountId::from_str(&account_id)
                .map_err({
                    let store = store.clone();
                    let idempotency_key = idempotency_key.clone();
                    move |_err| {
                    let error_msg = format!("Unable to parse account id: {}", account_id);
                    error!("{}", error_msg);
                    let status_code = StatusCode::from_u16(400).unwrap();
                    let data = Bytes::from(error_msg);
                    store.save_idempotent_data(idempotency_key, status_code, data.clone());
                    Response::builder().status(400).body(data).unwrap()
                }}))
                .and_then({
                    let store = store.clone();
                    let idempotency_key = idempotency_key.clone();
                    move |account_id|
                store.get_accounts(vec![account_id])
                .map_err({
                    move |_| {
                    let error_msg = format!("Error getting account: {}", account_id);
                    error!("{}", error_msg);
                    let status_code = StatusCode::from_u16(404).unwrap();
                    let data = Bytes::from(error_msg);
                    store.save_idempotent_data(idempotency_key, status_code, data.clone());
                    Response::builder().status(404).body(data).unwrap()
                }})})
                .and_then(|accounts| {
                    let account = &accounts[0];
                    if let Some(settlement_engine) = account.settlement_engine_details() {
                        Ok((account.clone(), settlement_engine))
                    } else {
                        let err = format!("Account {} has no settlement engine details configured, cannot send a settlement engine message to that account", accounts[0].id());
                        error!("{}", err);
                        Err(Response::builder().status(404).body(Bytes::from(err)).unwrap())
                    }
                })
                .and_then({
                    let store = store.clone();
                    let idempotency_key = idempotency_key.clone();
                    move |(account, settlement_engine)| {
                    // Send the message to the peer's settlement engine.
                    // Note that we use dummy values for the `from` and `original_amount`
                    // because this `OutgoingRequest` will bypass the router and thus will not
                    // use either of these values. Including dummy values in the rare case where
                    // we do not need them seems easier than using
                    // `Option`s all over the place.
                    outgoing_handler.send_request(OutgoingRequest {
                        from: account.clone(),
                        to: account.clone(),
                        original_amount: 0,
                        prepare: PrepareBuilder {
                            destination: settlement_engine.ilp_address,
                            amount: 0,
                            expires_at: SystemTime::now() + Duration::from_secs(30),
                            data: &body,
                            execution_condition: &PEER_PROTOCOL_CONDITION,
                        }.build()
                    })
                    .map_err({
                        move |reject| {
                        let error_msg = format!("Error sending message to peer settlement engine. Packet rejected with code: {}, message: {}", reject.code(), str::from_utf8(reject.message()).unwrap_or_default());
                        error!("{}", error_msg);
                        let data = Bytes::from(error_msg);
                        store.save_idempotent_data(idempotency_key, StatusCode::from_u16(502).unwrap(), data.clone());
                        Response::builder().status(502).body(data).unwrap()
                    }})
                }})
                .and_then({
                    move |fulfill| {
                    let data = Bytes::from(fulfill.data());
                    store.save_idempotent_data(idempotency_key, StatusCode::OK, data.clone())
                    .map_err(move |err| {
                        let err = format!("Couldn't connect to store {:?}", err);
                        error!("{}", err);
                        Response::builder().status(500).body(Bytes::from(err)).unwrap()
                    })
                    .and_then(|_| Ok(
                        Response::builder().status(200).body(data).unwrap()
                    ))
                }})
            )})
        }

        pub fn serve<I>(self, incoming: I) -> impl Future<Item = (), Error = ()>
        where
            I: ConnectionStream,
            I::Item: Send + 'static,
        {
            ServiceBuilder::new()
                .resource(self)
                .serve(incoming)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::*;
    use crate::test_helpers::*;

    // Settlement Tests
    mod settlement_tests {
        use super::*;

        #[test]
        fn settlement_ok() {
            let id = TEST_ACCOUNT_0.clone().id.to_string();
            let store = test_store(false, true);
            let api = test_api(store.clone(), false);

            let ret: Response<_> = api
                .receive_settlement(
                    id.clone(),
                    SettlementData {
                        amount: SETTLEMENT_BODY,
                    },
                    IDEMPOTENCY.to_string(),
                )
                .wait()
                .unwrap();
            assert_eq!(ret.status(), 200);
            assert_eq!(ret.body(), "Success");

            // check that it's idempotent
            let ret: Response<_> = api
                .receive_settlement(id, SettlementData { amount: 200 }, IDEMPOTENCY.to_string())
                .wait()
                .unwrap();
            assert_eq!(ret.status(), 200);
            assert_eq!(ret.body(), "Success");

            let s = store.clone();
            let cache = s.cache.read();
            let cached_data = cache.get(&IDEMPOTENCY.to_string()).unwrap();

            let cache_hits = s.cache_hits.read();
            assert_eq!(*cache_hits, 1);
            assert_eq!(cached_data.0, StatusCode::OK);
            assert_eq!(cached_data.1, &Bytes::from("Success"));
        }

        #[test]
        fn account_has_no_engine_configured() {
            let id = TEST_ACCOUNT_0.clone().id.to_string();
            let store = test_store(false, false);
            let api = test_api(store.clone(), false);

            let ret: Response<_> = api
                .receive_settlement(
                    id.clone(),
                    SettlementData {
                        amount: SETTLEMENT_BODY,
                    },
                    IDEMPOTENCY.to_string(),
                )
                .wait()
                .unwrap_err();
            assert_eq!(ret.status().as_u16(), 404);
            assert_eq!(ret.body(), "Account 0 does not have settlement engine details configured. Cannot handle incoming settlement");

            // check that it's idempotent
            let ret: Response<_> = api
                .receive_settlement(
                    id,
                    SettlementData {
                        amount: SETTLEMENT_BODY,
                    },
                    IDEMPOTENCY.to_string(),
                )
                .wait()
                .unwrap_err();
            assert_eq!(ret.status().as_u16(), 404);
            assert_eq!(ret.body(), "Account 0 does not have settlement engine details configured. Cannot handle incoming settlement");

            let s = store.clone();
            let cache = s.cache.read();
            let cached_data = cache.get(&IDEMPOTENCY.to_string()).unwrap();

            let cache_hits = s.cache_hits.read();
            assert_eq!(*cache_hits, 1);
            assert_eq!(cached_data.0, 404);
            assert_eq!(cached_data.1, &Bytes::from("Account 0 does not have settlement engine details configured. Cannot handle incoming settlement"));
        }

        #[test]
        fn update_balance_for_incoming_settlement_fails() {
            let id = TEST_ACCOUNT_0.clone().id.to_string();
            let store = test_store(true, true);
            let api = test_api(store, false);

            let ret: Response<_> = api
                .receive_settlement(
                    id,
                    SettlementData {
                        amount: SETTLEMENT_BODY,
                    },
                    IDEMPOTENCY.to_string(),
                )
                .wait()
                .unwrap_err();
            assert_eq!(ret.status().as_u16(), 500);
        }

        #[test]
        fn invalid_account_id() {
            // the api is configured to take an accountId type
            // supplying an id that cannot be parsed to that type must fail
            let id = "a".to_string();
            let store = test_store(false, true);
            let api = test_api(store.clone(), false);

            let ret: Response<_> = api
                .receive_settlement(
                    id.clone(),
                    SettlementData {
                        amount: SETTLEMENT_BODY,
                    },
                    IDEMPOTENCY.to_string(),
                )
                .wait()
                .unwrap_err();
            assert_eq!(ret.status().as_u16(), 400);
            assert_eq!(ret.body(), "Unable to parse account id: a");

            // check that it's idempotent
            let ret: Response<_> = api
                .receive_settlement(
                    id.clone(),
                    SettlementData { amount: 999 },
                    IDEMPOTENCY.to_string(),
                )
                .wait()
                .unwrap_err();
            assert_eq!(ret.status().as_u16(), 400);
            assert_eq!(ret.body(), "Unable to parse account id: a");

            let _ret: Response<_> = api
                .receive_settlement(id, SettlementData { amount: 999 }, IDEMPOTENCY.to_string())
                .wait()
                .unwrap_err();

            let s = store.clone();
            let cache = s.cache.read();
            let cached_data = cache.get(&IDEMPOTENCY.to_string()).unwrap();

            let cache_hits = s.cache_hits.read();
            assert_eq!(*cache_hits, 2);
            assert_eq!(cached_data.0, 400);
            assert_eq!(cached_data.1, &Bytes::from("Unable to parse account id: a"));
        }

        #[test]
        fn account_not_in_store() {
            let id = TEST_ACCOUNT_0.clone().id.to_string();
            let store = TestStore::new(vec![], false);
            let api = test_api(store.clone(), false);

            let ret: Response<_> = api
                .receive_settlement(
                    id.clone(),
                    SettlementData {
                        amount: SETTLEMENT_BODY,
                    },
                    IDEMPOTENCY.to_string(),
                )
                .wait()
                .unwrap_err();
            assert_eq!(ret.status().as_u16(), 404);
            assert_eq!(ret.body(), "Error getting account: 0");

            let ret: Response<_> = api
                .receive_settlement(
                    id,
                    SettlementData {
                        amount: SETTLEMENT_BODY,
                    },
                    IDEMPOTENCY.to_string(),
                )
                .wait()
                .unwrap_err(); // Bug that returns Result::OK
            assert_eq!(ret.status().as_u16(), 404);
            assert_eq!(ret.body(), "Error getting account: 0");

            let s = store.clone();
            let cache = s.cache.read();
            let cached_data = cache.get(&IDEMPOTENCY.to_string()).unwrap();
            let cache_hits = s.cache_hits.read();
            assert_eq!(*cache_hits, 1);
            assert_eq!(cached_data.0, 404);
            assert_eq!(cached_data.1, &Bytes::from("Error getting account: 0"));
        }
    }

    mod message_tests {
        use super::*;

        #[test]
        fn message_ok() {
            let id = TEST_ACCOUNT_0.clone().id.to_string();
            let store = test_store(false, true);
            let api = test_api(store.clone(), true);

            let ret: Response<_> = api
                .send_outgoing_message(id.clone(), vec![], IDEMPOTENCY.to_string())
                .wait()
                .unwrap();
            assert_eq!(ret.status(), StatusCode::OK);
            assert_eq!(ret.body(), &Bytes::from("hello!"));

            let ret: Response<_> = api
                .send_outgoing_message(id, vec![], IDEMPOTENCY.to_string())
                .wait()
                .unwrap();
            assert_eq!(ret.status(), StatusCode::OK);
            assert_eq!(ret.body(), &Bytes::from("hello!"));

            let s = store.clone();
            let cache = s.cache.read();
            let cached_data = cache.get(&IDEMPOTENCY.to_string()).unwrap();
            let cache_hits = s.cache_hits.read();
            assert_eq!(*cache_hits, 1);
            assert_eq!(cached_data.0, StatusCode::OK);
            assert_eq!(cached_data.1, &Bytes::from("hello!"));
        }

        #[test]
        fn message_gets_rejected() {
            let id = TEST_ACCOUNT_0.clone().id.to_string();
            let store = test_store(false, true);
            let api = test_api(store.clone(), false);

            let ret = api
                .send_outgoing_message(id.clone(), vec![], IDEMPOTENCY.to_string())
                .wait()
                .unwrap_err();
            assert_eq!(ret.status().as_u16(), 502);

            let ret = api
                .send_outgoing_message(id, vec![], IDEMPOTENCY.to_string())
                .wait()
                .unwrap_err();
            assert_eq!(ret.status().as_u16(), 502);

            let s = store.clone();
            let cache = s.cache.read();
            let cached_data = cache.get(&IDEMPOTENCY.to_string()).unwrap();
            let cache_hits = s.cache_hits.read();
            assert_eq!(*cache_hits, 1);
            assert_eq!(cached_data.0, 502);
            assert_eq!(cached_data.1, &Bytes::from("Error sending message to peer settlement engine. Packet rejected with code: F02, message: No other outgoing handler!"));
        }

        #[test]
        fn invalid_account_id() {
            // the api is configured to take an accountId type
            // supplying an id that cannot be parsed to that type must fail
            let id = "a".to_string();
            let store = test_store(false, true);
            let api = test_api(store.clone(), true);

            let ret: Response<_> = api
                .send_outgoing_message(id.clone(), vec![], IDEMPOTENCY.to_string())
                .wait()
                .unwrap_err();
            assert_eq!(ret.status().as_u16(), 400);

            let ret: Response<_> = api
                .send_outgoing_message(id.clone(), vec![], IDEMPOTENCY.to_string())
                .wait()
                .unwrap_err();
            assert_eq!(ret.status().as_u16(), 400);

            let _ret: Response<_> = api
                .send_outgoing_message(id, vec![], IDEMPOTENCY.to_string())
                .wait()
                .unwrap_err();
            assert_eq!(ret.status().as_u16(), 400);

            let s = store.clone();
            let cache = s.cache.read();
            let cached_data = cache.get(&IDEMPOTENCY.to_string()).unwrap();

            let cache_hits = s.cache_hits.read();
            assert_eq!(*cache_hits, 2);
            assert_eq!(cached_data.0, 400);
            assert_eq!(cached_data.1, &Bytes::from("Unable to parse account id: a"));
        }

        #[test]
        fn account_not_in_store() {
            let id = TEST_ACCOUNT_0.clone().id.to_string();
            let store = TestStore::new(vec![], false);
            let api = test_api(store.clone(), true);

            let ret: Response<_> = api
                .send_outgoing_message(id.clone(), vec![], IDEMPOTENCY.to_string())
                .wait()
                .unwrap_err();
            assert_eq!(ret.status().as_u16(), 404);

            let ret: Response<_> = api
                .send_outgoing_message(id, vec![], IDEMPOTENCY.to_string())
                .wait()
                .unwrap_err();
            assert_eq!(ret.status().as_u16(), 404);

            let s = store.clone();
            let cache = s.cache.read();
            let cached_data = cache.get(&IDEMPOTENCY.to_string()).unwrap();

            let cache_hits = s.cache_hits.read();
            assert_eq!(*cache_hits, 1);
            assert_eq!(cached_data.0, 404);
            assert_eq!(cached_data.1, &Bytes::from("Error getting account: 0"));
        }
    }
}
