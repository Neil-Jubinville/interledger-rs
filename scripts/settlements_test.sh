killall interledger node

# start ganache
# ganache-cli -m "abstract vacuum mammal awkward pudding scene penalty purchase dinner depart evoke puzzle" -i 1 &> /dev/null &

# sleep 3 # wait for ganache to start

ROOT=$HOME/projects/xpring-contract
ILP_DIR=$ROOT/interledger-rs
ILP=$ILP_DIR/target/debug/interledger


LOGS=`pwd`/settlement_test_logs
mkdir -p $LOGS

echo "Initializing redis"
bash $ILP_DIR/scripts/init.sh &

sleep 1

ALICE_ADDRESS="3cdb3d9e1b74692bb1e3bb5fc81938151ca64b02"
ALICE_KEY="380eb0f3d505f087e438eca80bc4df9a7faa24f868e69fc0440261a0fc0567dc"
BOB_ADDRESS="9b925641c5ef3fd86f63bff2da55a0deeafd1263"
BOB_KEY="cc96601bc52293b53c4736a12af9130abf347669b3813f9ec4cafdf6991b087e"


echo "Initializing Alice SE"
$ILP settlement-engine ethereum-ledger \
--key $ALICE_KEY \
--server_secret aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa \
--confirmations 0 \
--poll_frequency 1 \
--ethereum_endpoint http://127.0.0.1:8545 \
--connector_url http://127.0.0.1:7771 \
--redis_uri redis://127.0.0.1:6379 \
--port 3000 > $LOGS/engine_alice.log &

echo "Initializing Bob SE"
$ILP settlement-engine ethereum-ledger \
--key $BOB_KEY \
--server_secret bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb \
--confirmations 0 \
--poll_frequency 1 \
--ethereum_endpoint http://127.0.0.1:8545 \
--connector_url http://127.0.0.1:8771 \
--redis_uri redis://127.0.0.1:6380 \
--port 3001 > $LOGS/engine_bob.log &

sleep 1

echo "Initializing Alice Connector"
$ILP node --config $ILP_DIR/configs/alice.yaml &> $LOGS/ilp_alice.log &
echo "Initializing Bob Connector"
$ILP node --config $ILP_DIR/configs/bob.yaml &> $LOGS/ilp_bob.log &

sleep 1


# insert Bob's account details on Alice's connector
curl http://localhost:7770/accounts -X POST \
-d "ilp_address=example.bob&asset_code=ETH&asset_scale=18&max_packet_amount=1&settlement_engine_url=http://127.0.0.1:3000&settlement_engine_asset_scale=9&settlement_engine_ilp_address=peer.settle.xrpl&http_endpoint=http://127.0.0.1:8770/ilp&http_incoming_token=bob&http_outgoing_token=alice" \
-H "Authorization: Bearer hi_alice"

# insert Alice's account details on Bob's connector
curl http://localhost:8770/accounts -X POST \
-d "ilp_address=example.alice&asset_code=xyz&asset_scale=9&max_packet_amount=1&settlement_engine_url=http://127.0.0.1:3001&settlement_engine_asset_scale=9&settlement_engine_ilp_address=peer.settle.xrpl&http_endpoint=http://127.0.0.1:7770/ilp&http_incoming_token=alice&http_outgoing_token=bob" \
-H "Authorization: Bearer hi_bob"

echo '\nsetting up bob's eth address on alice's engine'
curl -vvv 127.0.0.1:3000/accounts/0 \
--trace-ascii /dev/stdout \
-H "Content-Type: application/json" \
-d "{ \"own_address\" : \"$BOB_ADDRESS\"}" #, \"token_address\": \"null\" }"

echo '\nsetting up alice's eth address on bob's engine'
curl -vvv 127.0.0.1:3001/accounts/0 \
--trace-ascii /dev/stdout \
-H "Content-Type: application/json" \
-d "{ \"own_address\" : \"$ALICE_ADDRESS\"}" #", \"token_address\": \"null\" }"

sleep 2

# the proper keys should be set
echo "Alice Store:"
redis-cli -p 6379 keys "*"
echo "Bob Store:"
redis-cli -p 6380 keys "*"

# bob must pay alice
# A /settlements request is made by Bob's connector for account 0 to Bob's engine
# (todo: make this happen over spsp/stream, talk with evan about it)
# echo 'simulating settlement where Bob pays Alice'
curl localhost:3001/accounts/0/settlement -d "scale=6&amount=696969"

cat $LOGS/engine_bob.log
