#![recursion_limit = "128"]
#[macro_use]
extern crate log;

#[macro_use]
#[cfg(test)]
extern crate lazy_static;

#[macro_use]
extern crate serde_json;

#[cfg(test)]
mod test_helpers;

#[cfg(test)]
mod fixtures;

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
    fn ethereum_address(&self) -> Address;

    fn token_address(&self) -> Option<Address> {
        None
    }
}

/// First element is the account's ethereum adddress
/// second element is the account's erc20 token if it's some, otherwise it means
/// ethereum.
pub type Addresses = (Address, Option<Address>);

pub trait EthereumStore {
    type Account: EthereumAccount;

    /// Saves the Ethereum address associated with this account
    /// called when creating an account on the API
    fn save_account_addresses(
        &self,
        account_ids: Vec<<Self::Account as Account>::AccountId>,
        data: Vec<Addresses>,
    ) -> Box<dyn Future<Item = (), Error = ()> + Send>;

    /// Loads the Ethereum address associated with this account
    fn load_account_addresses(
        &self,
        account_ids: Vec<<Self::Account as Account>::AccountId>,
    ) -> Box<dyn Future<Item = Vec<Addresses>, Error = ()> + Send>;
}
