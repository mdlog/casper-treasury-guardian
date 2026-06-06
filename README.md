# Treasury Guardian

**An on-chain policy vault that lets an autonomous AI agent manage a treasury — safely.**

Built for the [Casper Agentic Buildathon](https://dorahacks.io/hackathon/casper-agentic-buildathon/buidl).

The agent can *propose* spends and pay for data per-call via **x402**, but a
**Casper smart contract enforces the guardrails** (daily caps, per-category caps,
allowlists, an auto-execute threshold, and a cooldown). The agent never holds the
treasury keys — the vault is the trust boundary. This is the *"trust layer for the
agent economy"* made concrete.

```
                 proposes                 enforces policy
   AI Agent  ───────────────►  Policy Vault (Casper)  ───────────────►  executes / queues / reverts
  (off-chain)   propose_action      Odra smart contract      on-chain transfer · human approval · revert
       │
       └── pays per API call via x402 (settled through the same vault)
```

## The core idea: Propose → Enforce → Execute

Every action the agent wants to take flows through one contract entry point,
`propose_action`. The contract decides the outcome **on-chain**:

| Situation | Outcome |
|---|---|
| Small, allowlisted, within caps | **Auto-executed** immediately |
| Above the auto-threshold, or recipient not allowlisted | **Queued** for the owner to approve / veto |
| Would exceed a daily/category cap, or cooldown active | **Reverted** — the agent simply cannot do it |

A hallucinating or prompt-injected agent therefore cannot drain the treasury: the
worst it can do is propose actions the contract refuses or holds for a human.

## Repository layout

| Path | What it is |
|---|---|
| [`contract/`](contract/) | The Odra (Rust) **Policy Vault** smart contract + 14 unit tests, the testnet deploy binary, and `tg_cli` (the on-chain gateway). |
| [`agent/`](agent/) | TypeScript **agent runtime**: buys market data via x402, reasons about risk, and proposes actions through the vault. |
| [`mock-api/`](mock-api/) | A **premium API gated by x402** — what the agent pays for, per call. |
| [`dashboard/`](dashboard/) | React **owner console**: vault state, policy editor, approve/veto queue, audit log. |
| [`cspr_proxy.js`](cspr_proxy.js) | CSPR.cloud auth proxy for deploying via Odra livenet. |
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | Design, contract API, and security model. |
| [`DEPLOY.md`](DEPLOY.md) | Step-by-step Casper Testnet deployment. |

## Quick start

### 1. Contract: build & test

```bash
cd contract
cargo test                 # 14 unit tests (Odra HostEnv) — policy logic
cargo odra build           # produces wasm/TreasuryGuardian.wasm
```

### 2. Mock x402 API

```bash
cd mock-api && npm install && npm start
# GET http://localhost:4021/api/market-intel  →  402 until paid
```

### 3. Agent (offline demo, no chain needed)

```bash
cd agent && npm install
npm run dry-run            # one cycle: x402 handshake + reasoning, settlement simulated
```

You'll see the agent receive a `402`, "settle" the payment, fetch the premium
market intel, score treasury risk, and decide on a rebalancing action.

### 4. Deploy to Casper Testnet & go live

See [`DEPLOY.md`](DEPLOY.md). In short: generate a key, fund it at the
[faucet](https://testnet.cspr.live/tools/faucet), add a CSPR.cloud token, then:

```bash
node cspr_proxy.js                                            # terminal 1
cd contract && cargo run --bin deploy_testnet --features=livenet   # terminal 2
```

Put the printed contract package hash into `.env` as `TG_CONTRACT_ADDRESS`, then:

```bash
cd dashboard && npm install && npm start    # owner console at http://localhost:5173
cd agent && npm start                        # agent proposes real on-chain actions
```

## What's implemented

- **Smart contract** (`contract/src/treasury_guardian.rs`): custody (`deposit`/`withdraw`),
  `propose_action` with full decision routing, `approve`/`veto`, policy setters,
  rolling-24h window accounting, events, and typed errors. 14 passing tests.
- **x402** end-to-end: the mock API returns `402` + `PaymentRequirements`; the agent
  settles through the vault and retries with an `X-PAYMENT` header; the API verifies
  and serves the data.
- **Agent runtime**: reads vault state, runs a (rule-based, LLM-ready) planner, and
  proposes actions — observing the contract's auto/queue/revert decision.
- **Owner dashboard**: live vault state, policy editing, and the approve/veto queue.

See [`ARCHITECTURE.md`](ARCHITECTURE.md) for the contract API and security model.

## License

MIT — see [`LICENSE`](LICENSE).
