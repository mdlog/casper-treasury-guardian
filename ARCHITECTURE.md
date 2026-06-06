# Architecture & Security Model

## Components

```
┌──────────────┐     402 + requirements      ┌──────────────┐
│  mock-api    │ ◄────────────────────────── │              │
│ (x402 gate)  │ ──────────────────────────► │    agent     │  reasons about treasury risk,
└──────────────┘     data (after payment)    │ (TypeScript) │  proposes actions
                                              └──────┬───────┘
                                                     │ propose_action / view  (via tg_cli)
                                                     ▼
┌──────────────┐    approve / veto / policy   ┌──────────────────────────┐
│  dashboard   │ ───────────────────────────► │  Policy Vault (Odra/Rust) │  ENFORCES policy on-chain
│  (owner UI)  │ ◄─────────────────────────── │     Casper Testnet        │  holds the treasury
└──────────────┘        vault state            └──────────────────────────┘
```

### Three identities
- **Owner** — a human (or multisig). Sets policy, approves/vetoes queued actions, withdraws.
- **Agent** — the autonomous program. The *only* account allowed to call `propose_action`. Holds **no** authority over funds beyond proposing.
- **Vault** — the contract itself, which custodies the treasury and is the trust boundary.

### Why a Rust `tg_cli` gateway?
Both the agent and the dashboard call on-chain through `contract/bin/tg_cli.rs`
(an Odra *livenet* binary). This reuses Odra's runtime-argument encoding and
signing so we never re-implement Casper serialization in JavaScript, and keeps
one audited path to the chain. It prints machine-readable `TG_JSON {...}` lines
that the Node processes parse.

## Contract API (`treasury_guardian::TreasuryGuardian`)

### Custody
- `deposit()` *(payable)* — fund the vault; emits `Deposited`.
- `withdraw(amount, to)` *(owner)* — pull funds out; emits `Withdrawn`.

### The one agent entry point
- `propose_action(kind, recipient, amount, category, memo) -> Decision`
  *(agent only)* — emits `ActionProposed`, then routes:
  - **`Decision::AutoExecuted`** — within threshold, allowlisted, within caps,
    cooldown satisfied → transfers immediately; emits `ActionExecuted`.
  - **`Decision::Pending`** — above `auto_threshold` *or* recipient not on the
    allowlist → enqueued; emits `ActionQueued{reason}`.
  - **revert** — would exceed a daily/category cap (`CapExceeded`) or cooldown
    active (`CooldownActive`). The agent is simply blocked.

### Owner controls
- `approve(id)` — execute a queued action (emits `ActionApproved` + `ActionExecuted`).
- `veto(id)` — reject it permanently (emits `ActionVetoed`).
- `set_daily_cap`, `set_auto_threshold`, `set_category_cap`, `set_allowlist`,
  `set_require_allowlist`, `set_cooldown`, `set_agent`, `pause` — all emit `PolicyUpdated`/`PausedSet`.

### Views
`get_owner`, `get_agent`, `is_paused`, `get_balance`, `get_daily_cap`,
`get_auto_threshold`, `get_cooldown`, `requires_allowlist`, `is_allowlisted`,
`get_category_cap`, `spent_today`, `remaining_today`, `next_id`, `get_pending`.

### Errors
`NotOwner`, `NotAgent`, `Paused`, `CapExceeded`, `CooldownActive`,
`InsufficientFunds`, `UnknownPending`, `AlreadyExecuted`, `AlreadyVetoed`, `ZeroAmount`.

## Rolling 24h window
`spent_today` (and per-category counters) reset when `now - window_start >= 24h`.
Caps are evaluated against the current window inside `would_exceed_caps`, so the
daily cap is a true rolling limit rather than a calendar-day reset.

## x402 flow (native CSPR settlement)
1. Agent `GET`s the resource → `402` + `PaymentRequirements{ payTo, maxAmountRequired, … }`.
2. Agent calls `propose_action(X402Payment, payTo, amount, "data", …)`.
   - The vault enforces the **same** caps/allowlist as any other spend, so API
     spending is bounded and auditable. `"data"` is its own category, so the
     owner can cap data spend independently.
3. On `AutoExecuted`, the on-chain transfer is the proof of payment. The agent
   retries with an `X-PAYMENT` header carrying the settlement evidence; the API
   verifies and returns the data + `X-PAYMENT-RESPONSE`.

## Security model

| Threat | Mitigation |
|---|---|
| Agent hallucination / prompt injection tries to over-spend | Caps + threshold + allowlist enforced **on-chain**; large/unknown spends only queue, never auto-execute. |
| Compromised agent key | Agent can only *propose*. It cannot withdraw, change policy, or move funds outside caps/allowlist. |
| Runaway loop draining via many small spends | Rolling daily cap + per-category caps + cooldown between auto-executions. |
| Bad/unknown counterparty | `require_allowlist` forces unknown recipients into the human approval queue. |
| Emergency | Owner can `pause()` to halt all proposals, and `veto()` any queued action. |
| Off-chain components buggy | They hold no authority; the contract is the source of truth. Worst case the agent stops proposing. |

The agent is **untrusted by design**. Safety comes from the contract, not from
trusting the model.
