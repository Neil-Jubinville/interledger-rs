#[macro_use]
extern crate lazy_static;

mod common;

use bytes::Bytes;
use common::*;
use ethereum_tx_sign::web3::types::Address as EthAddress;
use http::StatusCode;
use interledger_settlement::{IdempotentStore, SettlementStore};
use interledger_settlement_engines::EthereumStore;
use redis::{cmd, r#async::SharedConnection};
use std::str::FromStr;

lazy_static! {
    static ref IDEMPOTENCY_KEY: String = String::from("AJKJNUjM0oyiAN46");
}

#[test]
fn credits_prepaid_amount() {
    block_on(test_store().and_then(|(store, context)| {
        context.async_connection().and_then(move |conn| {
            store
                .update_balance_for_incoming_settlement(0, 100, Some(IDEMPOTENCY_KEY.clone()))
                .and_then(move |_| {
                    cmd("HMGET")
                        .arg("accounts:0")
                        .arg("balance")
                        .arg("prepaid_amount")
                        .query_async(conn)
                        .map_err(|err| eprintln!("Redis error: {:?}", err))
                        .and_then(move |(_conn, (balance, prepaid_amount)): (_, (i64, i64))| {
                            assert_eq!(balance, 0);
                            assert_eq!(prepaid_amount, 100);
                            let _ = context;
                            Ok(())
                        })
                })
        })
    }))
    .unwrap()
}

#[test]
fn saves_and_loads_ethereum_addreses_properly() {
    block_on(test_store().and_then(|(store, context)| {
        let account_ids = vec![0, 1];
        let account_addresses = vec![
            (
                EthAddress::from_str("3cdb3d9e1b74692bb1e3bb5fc81938151ca64b02").unwrap(),
                Some(EthAddress::from_str("c92be489639a9c61f517bd3b955840fa19bc9b7c").unwrap()),
            ),
            (
                EthAddress::from_str("2fcd07047c209c46a767f8338cb0b14955826826").unwrap(),
                None,
            ),
        ];
        store
            .save_account_addresses(account_ids.clone(), account_addresses.clone())
            .map_err(|err| eprintln!("Redis error: {:?}", err))
            .and_then(move |_| {
                store
                    .load_account_addresses(account_ids)
                    .map_err(|err| eprintln!("Redis error: {:?}", err))
                    .and_then(move |data| {
                        assert_eq!(data[0], account_addresses[0]);
                        assert_eq!(data[1], account_addresses[1]);
                        let _ = context;
                        Ok(())
                    })
            })
    }))
    .unwrap()
}

#[test]
fn saves_and_loads_idempotency_key_data_properly() {
    block_on(test_store().and_then(|(store, context)| {
        let input_hash: [u8; 32] = Default::default();
        store
            .save_idempotent_data(
                Some(IDEMPOTENCY_KEY.clone()),
                input_hash,
                StatusCode::OK,
                Bytes::from("TEST"),
            )
            .map_err(|err| eprintln!("Redis error: {:?}", err))
            .and_then(move |_| {
                store
                    .load_idempotent_data(Some(IDEMPOTENCY_KEY.clone()))
                    .map_err(|err| eprintln!("Redis error: {:?}", err))
                    .and_then(move |data1| {
                        assert_eq!(
                            data1.unwrap(),
                            (StatusCode::OK, Bytes::from("TEST"), input_hash)
                        );
                        let _ = context;

                        store
                            .load_idempotent_data(Some("asdf".to_string()))
                            .map_err(|err| eprintln!("Redis error: {:?}", err))
                            .and_then(move |data2| {
                                assert!(data2.is_none());
                                let _ = context;
                                Ok(())
                            })
                    })
            })
    }))
    .unwrap()
}

#[test]
fn idempotent_settlement_calls() {
    block_on(test_store().and_then(|(store, context)| {
        context.async_connection().and_then(move |conn| {
            store
                .update_balance_for_incoming_settlement(0, 100, Some(IDEMPOTENCY_KEY.clone()))
                .and_then(move |_| {
                    cmd("HMGET")
                        .arg("accounts:0")
                        .arg("balance")
                        .arg("prepaid_amount")
                        .query_async(conn)
                        .map_err(|err| eprintln!("Redis error: {:?}", err))
                        .and_then(move |(conn, (balance, prepaid_amount)): (_, (i64, i64))| {
                            assert_eq!(balance, 0);
                            assert_eq!(prepaid_amount, 100);

                            store
                                .update_balance_for_incoming_settlement(
                                    0,
                                    100,
                                    Some(IDEMPOTENCY_KEY.clone()), // Reuse key to make idempotent request.
                                )
                                .and_then(move |_| {
                                    cmd("HMGET")
                                        .arg("accounts:0")
                                        .arg("balance")
                                        .arg("prepaid_amount")
                                        .query_async(conn)
                                        .map_err(|err| eprintln!("Redis error: {:?}", err))
                                        .and_then(
                                            move |(_conn, (balance, prepaid_amount)): (
                                                _,
                                                (i64, i64),
                                            )| {
                                                // Since it's idempotent there
                                                // will be no state update.
                                                // Otherwise it'd be 200 (100 + 100)
                                                assert_eq!(balance, 0);
                                                assert_eq!(prepaid_amount, 100);
                                                let _ = context;
                                                Ok(())
                                            },
                                        )
                                })
                        })
                })
        })
    }))
    .unwrap()
}

#[test]
fn credits_balance_owed() {
    block_on(test_store().and_then(|(store, context)| {
        context
            .shared_async_connection()
            .map_err(|err| panic!(err))
            .and_then(move |conn| {
                cmd("HSET")
                    .arg("accounts:0")
                    .arg("balance")
                    .arg(-200)
                    .query_async(conn)
                    .map_err(|err| panic!(err))
                    .and_then(move |(conn, _balance): (SharedConnection, i64)| {
                        store
                            .update_balance_for_incoming_settlement(
                                0,
                                100,
                                Some(IDEMPOTENCY_KEY.clone()),
                            )
                            .and_then(move |_| {
                                cmd("HMGET")
                                    .arg("accounts:0")
                                    .arg("balance")
                                    .arg("prepaid_amount")
                                    .query_async(conn)
                                    .map_err(|err| panic!(err))
                                    .and_then(
                                        move |(_conn, (balance, prepaid_amount)): (
                                            _,
                                            (i64, i64),
                                        )| {
                                            assert_eq!(balance, -100);
                                            assert_eq!(prepaid_amount, 0);
                                            let _ = context;
                                            Ok(())
                                        },
                                    )
                            })
                    })
            })
    }))
    .unwrap()
}

#[test]
fn clears_balance_owed() {
    block_on(test_store().and_then(|(store, context)| {
        context
            .shared_async_connection()
            .map_err(|err| panic!(err))
            .and_then(move |conn| {
                cmd("HSET")
                    .arg("accounts:0")
                    .arg("balance")
                    .arg(-100)
                    .query_async(conn)
                    .map_err(|err| panic!(err))
                    .and_then(move |(conn, _balance): (SharedConnection, i64)| {
                        store
                            .update_balance_for_incoming_settlement(
                                0,
                                100,
                                Some(IDEMPOTENCY_KEY.clone()),
                            )
                            .and_then(move |_| {
                                cmd("HMGET")
                                    .arg("accounts:0")
                                    .arg("balance")
                                    .arg("prepaid_amount")
                                    .query_async(conn)
                                    .map_err(|err| panic!(err))
                                    .and_then(
                                        move |(_conn, (balance, prepaid_amount)): (
                                            _,
                                            (i64, i64),
                                        )| {
                                            assert_eq!(balance, 0);
                                            assert_eq!(prepaid_amount, 0);
                                            let _ = context;
                                            Ok(())
                                        },
                                    )
                            })
                    })
            })
    }))
    .unwrap()
}

#[test]
fn clears_balance_owed_and_puts_remainder_as_prepaid() {
    block_on(test_store().and_then(|(store, context)| {
        context
            .shared_async_connection()
            .map_err(|err| panic!(err))
            .and_then(move |conn| {
                cmd("HSET")
                    .arg("accounts:0")
                    .arg("balance")
                    .arg(-40)
                    .query_async(conn)
                    .map_err(|err| panic!(err))
                    .and_then(move |(conn, _balance): (SharedConnection, i64)| {
                        store
                            .update_balance_for_incoming_settlement(
                                0,
                                100,
                                Some(IDEMPOTENCY_KEY.clone()),
                            )
                            .and_then(move |_| {
                                cmd("HMGET")
                                    .arg("accounts:0")
                                    .arg("balance")
                                    .arg("prepaid_amount")
                                    .query_async(conn)
                                    .map_err(|err| panic!(err))
                                    .and_then(
                                        move |(_conn, (balance, prepaid_amount)): (
                                            _,
                                            (i64, i64),
                                        )| {
                                            assert_eq!(balance, 0);
                                            assert_eq!(prepaid_amount, 60);
                                            let _ = context;
                                            Ok(())
                                        },
                                    )
                            })
                    })
            })
    }))
    .unwrap()
}
