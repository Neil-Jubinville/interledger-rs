use ethereum_tx_sign::web3::{futures::future::Future, types::U256};
use std::time::Duration;
use tokio::runtime::Runtime;

mod lib;
use lib::EthereumSettlementEngine;

fn main() {
    // PARAMS SET IN CONFIG FILE
    let endpoint = "http://localhost:8545";
    let key = "380eb0f3d505f087e438eca80bc4df9a7faa24f868e69fc0440261a0fc0567dc";
    let addr = "3cdb3d9e1b74692bb1e3bb5fc81938151ca64b02".parse().unwrap();
    let chain_id = 1;
    let confs = 3;
    let poll_frequency = Duration::from_secs(5);

    // init
    let engine = EthereumSettlementEngine::new(
        endpoint.to_string(),
        key,
        addr,
        chain_id,
        confs,
        poll_frequency,
    );

    let bob = "2fcd07047c209c46a767f8338cb0b14955826826".parse().unwrap();
    let amount = 100000;
    // alice sends a transaction to bob
    let receipt = engine
        .settle_to(bob, U256::from(amount))
        .and_then(|receipt| {
            println!("RECEIPT {:?}", receipt);
            Ok(())
            // engine.notify_settlement(receipt.)
        });

    // Create the runtime
    let rt = Runtime::new().unwrap();
    let executor = rt.executor();
    // Spawn a future onto the runtime
    executor.spawn(receipt);
}