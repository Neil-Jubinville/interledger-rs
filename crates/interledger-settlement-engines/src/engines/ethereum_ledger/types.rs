use ethereum_tx_sign::{
    web3::types::{Address, H256},
    RawTransaction,
};
use futures::Future;
use interledger_service::Account;
use std::str::FromStr;
use ethkey::KeyPair;

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

/// Trait whcih can be implemented for other types such as HSMs to be used with
/// the SE.
pub trait EthereumLedgerTxSigner {
    /// Takes a transaction and returns an RLP encoded signed version of it
    fn sign(&self, tx: RawTransaction, chain_id: u8) -> Vec<u8>;

    fn address(&self) -> Address;
}

impl EthereumLedgerTxSigner for String {
    fn sign(&self, tx: RawTransaction, chain_id: u8) -> Vec<u8> {
        tx.sign(&H256::from_str(self).unwrap(), &chain_id)
    }

    fn address(&self) -> Address {
        let keypair = KeyPair::from_secret_slice(self.as_ref()).unwrap();
        // Convert to string and back to Address due to using different versions
        // of `ethereum_types` and `primitive_types`.
        let addr = ethkey::public_to_address(&keypair.public()).to_string();
        Address::from_str(&addr).unwrap()
    }
}
