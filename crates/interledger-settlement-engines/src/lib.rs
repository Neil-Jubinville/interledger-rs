#![recursion_limit = "128"]
#[macro_use]
extern crate log;

#[macro_use]
extern crate tower_web;

extern crate ethabi;

mod eth_engine;
mod helpers;

use ethereum_tx_sign::web3::types::Address;
use futures::Future;
use interledger_service::Account;

pub use self::eth_engine::EthereumSettlementEngine;
pub use self::helpers::{make_tx, TxSigner};

pub trait EthereumAccount: Account {
    fn token_adddress(&self) -> Option<Address> {
        None
    }
}

pub trait EthereumStore {
    type Account: EthereumAccount;

    /// Saves the Ethereum address associated with this account
    /// called when creating an account on the API
    fn save_account_address(
        &self,
        account_id: <Self::Account as Account>::AccountId,
        address: Address,
        token_address: Option<Address>,
    ) -> Box<dyn Future<Item = (), Error = ()> + Send>;

    /// Loads the Ethereum address associated with this account
    fn load_account_addresses(
        &self,
        account_id: Vec<<Self::Account as Account>::AccountId>,
    ) -> Box<dyn Future<Item = Vec<(Address, Address)>, Error = ()> + Send>;
}
