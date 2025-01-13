cargo build --locked --target wasm32-unknown-unknown -p bob_miner_v2 --release
ic-wasm target/wasm32-unknown-unknown/release/bob_miner_v2.wasm -o target/wasm32-unknown-unknown/release/bob_miner_v2.wasm metadata candid:service -f miner-v2/miner.did -v public

cargo build --locked --target wasm32-unknown-unknown -p bob-minter-v2 --release
ic-wasm target/wasm32-unknown-unknown/release/bob-minter-v2.wasm -o target/wasm32-unknown-unknown/release/bob-minter-v2.wasm metadata candid:service -f minter-v2/bob.did -v public
gzip -nf9 target/wasm32-unknown-unknown/release/bob-minter-v2.wasm

cargo build --locked --target wasm32-unknown-unknown -p bob-pool --release
ic-wasm target/wasm32-unknown-unknown/release/bob-pool.wasm -o target/wasm32-unknown-unknown/release/bob-pool.wasm metadata candid:service -f pool/pool.did -v public
gzip -nf9 target/wasm32-unknown-unknown/release/bob-pool.wasm
