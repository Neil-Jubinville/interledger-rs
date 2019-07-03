use super::*;
use futures::{
    future::{err, ok},
    Future,
};
use interledger_service::{Account, AccountStore};

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;



use std::process::Command;
use std::thread::sleep;
use std::time::Duration;
use tokio::runtime::Runtime;
use super::fixtures::TEST_ACCOUNT_0;


#[derive(Debug, Clone)]
pub struct TestAccount {
    pub id: u64,
    pub token_address: Address,
    pub no_details: bool,
}

impl Account for TestAccount {
    type AccountId = u64;

    fn id(&self) -> u64 {
        self.id
    }
}

impl EthereumAccount for TestAccount {
    fn token_adddress(&self) -> Option<Address> {
        if self.no_details {
            return None;
        }
        Some(self.token_address)
    }
}


// Test Store
#[derive(Clone)]
pub struct TestStore {
    pub accounts: Arc<Vec<TestAccount>>,
    pub should_fail: bool,
    pub addresses: Arc<RwLock<HashMap<u64, Addresses>>>,
    // pub cache: Arc<RwLock<HashMap<String, (StatusCode, Bytes)>>>,
    // pub cache_hits: Arc<RwLock<u64>>
}

impl EthereumStore for TestStore {
    type Account = TestAccount;

    fn save_account_addresses(
        &self,
        _account_ids: Vec<u64>,
        _data: Vec<Addresses>,
    ) -> Box<Future<Item = (), Error = ()> + Send> {
        // TODO
        // for (acc, d) in account_ids.into_iter().zip(data.into_iter()) {
        // let mut guard = self.addresses.write();
        // *guard.insert(acc, d);
        // }
        Box::new(ok(()))
    }

    fn load_account_addresses(
        &self,
        account_ids: Vec<u64>,
    ) -> Box<dyn Future<Item = Vec<Option<Addresses>>, Error = ()> + Send> {
        let mut v = Vec::with_capacity(account_ids.len());
        let addresses = self.addresses.read();
        for (i, acc) in account_ids.iter().enumerate() {
            if let Some(d) = addresses.get(&acc) {
                v[i] = Some((d.0, d.1));
            } else {
                v[i] = None;
            }
        }
        Box::new(ok(v))
    }
}

impl AccountStore for TestStore {
    type Account = TestAccount;

    fn get_accounts(
        &self,
        account_ids: Vec<<<Self as AccountStore>::Account as Account>::AccountId>,
    ) -> Box<Future<Item = Vec<Self::Account>, Error = ()> + Send> {
        let accounts: Vec<TestAccount> = self
            .accounts
            .iter()
            .filter_map(|account| {
                if account_ids.contains(&account.id) {
                    Some(account.clone())
                } else {
                    None
                }
            })
            .collect();
        if accounts.len() == account_ids.len() {
            Box::new(ok(accounts))
        } else {
            Box::new(err(()))
        }
    }
}

impl TestStore {
    pub fn new(accs: Vec<TestAccount>, should_fail: bool) -> Self {
        TestStore {
            accounts: Arc::new(accs),
            should_fail,
            addresses: Arc::new(RwLock::new(HashMap::new())),
            // cache: Arc::new(RwLock::new(HashMap::new())),
            // cache_hits: Arc::new(RwLock::new(0)),
        }
    }
}

// Test Service

impl TestAccount {
    pub fn new(id: u64, token_address: &str) -> Self {
        Self {
            id,
            token_address: Address::from_str(token_address).unwrap(),
            no_details: false,
        }
    }
}

// Futures helper taken from the store_helpers in interledger-store-redis.
pub fn block_on<F>(f: F) -> Result<F::Item, F::Error>
where
    F: Future + Send + 'static,
    F::Item: Send,
    F::Error: Send,
{
    let mut runtime = Runtime::new().unwrap();
    runtime.block_on(f)
}

// Helper to create a new engine and spin a new ganache instance.
pub fn test_engine<Si, S, A>(
    store: S,
    key: Si,
    addr: &str,
    confs: usize,
) -> (EthereumSettlementEngine<S, Si, A>, std::process::Child)
where
    Si: TxSigner + Clone + Send + Sync + 'static,
    S: EthereumStore<Account = A> + Clone + Send + Sync + 'static,
    A: EthereumAccount + Send + Sync + 'static,
{
    let mut ganache = Command::new("ganache-cli");
    let ganache = ganache.stdout(std::process::Stdio::null()).arg("-m").arg(
        "abstract vacuum mammal awkward pudding scene penalty purchase dinner depart evoke puzzle",
    );
    let ganache_pid = ganache.spawn().expect("couldnt start ganache-cli");
    // wait a couple of seconds for ganache to boot up
    sleep(Duration::from_secs(3));
    let chain_id = 1;
    let poll_frequency = Duration::from_secs(1);
    let engine = EthereumSettlementEngine::new(
        "http://localhost:8545".to_string(),
        store,
        key,
        Address::from_str(addr).unwrap(),
        chain_id,
        confs,
        poll_frequency,
    );

    (engine, ganache_pid)
}

pub fn test_store(store_fails: bool, account_has_engine: bool) -> TestStore {
    let mut acc = TEST_ACCOUNT_0.clone();
    acc.no_details = !account_has_engine;

    TestStore::new(vec![acc], store_fails)
}