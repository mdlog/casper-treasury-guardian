# Deploying to Casper Testnet

The vault deploys with Odra's *livenet* environment. CSPR.cloud's node RPC needs
an `Authorization` header that Odra can't attach directly, so a tiny local proxy
([`cspr_proxy.js`](cspr_proxy.js)) adds it.

## Prerequisites

- Rust toolchain + `cargo-odra` (see `contract/` for the pinned nightly).
- Node.js 18+.
- A CSPR.cloud API token — create one at <https://console.cspr.build/>.

## 1. Generate and fund a testnet key

```bash
casper-client keygen ./keys
# prints ./keys/secret_key.pem, public_key.pem, public_key_hex
cat ./keys/public_key_hex
```

Fund the public key (1000 CSPR) at the faucet:
<https://testnet.cspr.live/tools/faucet>

## 2. Configure `.env`

```bash
cp .env.example .env
```

Fill in:

```ini
ODRA_CASPER_LIVENET_SECRET_KEY_PATH=./keys/secret_key.pem
ODRA_CASPER_LIVENET_NODE_ADDRESS=http://127.0.0.1:7778/rpc
ODRA_CASPER_LIVENET_CHAIN_NAME=casper-test
CSPR_CLOUD_AUTH_TOKEN=<your CSPR.cloud token>
```

## 3. Deploy

In two terminals:

```bash
# terminal 1 — keep running
node cspr_proxy.js

# terminal 2
cd contract
cargo run --bin deploy_testnet --features=livenet
```

Optional policy at deploy time (motes are derived from CSPR):

```bash
TG_DAILY_CAP_CSPR=1000 TG_AUTO_THRESHOLD_CSPR=100 TG_COOLDOWN_MS=0 \
  cargo run --bin deploy_testnet --features=livenet
```

The command prints the deployed **contract package hash**. Copy it into `.env`:

```ini
TG_CONTRACT_ADDRESS=contract-package-xxxxxxxx...
```

## 4. Drive it on-chain with `tg_cli`

Everything below issues real testnet transactions (proxy must be running):

```bash
cd contract
cargo run -q --bin tg_cli --features=livenet -- state
cargo run -q --bin tg_cli --features=livenet -- deposit 200
cargo run -q --bin tg_cli --features=livenet -- allowlist <provider_pubkey_hex> true
cargo run -q --bin tg_cli --features=livenet -- propose native <pubkey_hex> 25 vendor "demo"
cargo run -q --bin tg_cli --features=livenet -- propose native <pubkey_hex> 500 vendor "big"  # -> Pending
cargo run -q --bin tg_cli --features=livenet -- approve 1
```

## 5. Run the live demo

```bash
# owner console (also starts its backend, which uses the same .env)
cd dashboard && npm install && npm start      # http://localhost:5173

# the autonomous agent (proposes real actions through the vault)
cd agent && npm install && npm start
```

## Notes
- `tg_cli` reads the same `.env`, so the agent and dashboard transact as the
  configured key. In production the owner would sign approvals in-wallet via
  CSPR.click; the agent would use a separate, lower-privilege key.
- Each `tg_cli` invocation builds (cached after the first) and submits one
  transaction; expect a few seconds per call while the deploy finalizes.
