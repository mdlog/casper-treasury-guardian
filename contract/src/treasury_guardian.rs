//! Treasury Guardian — on-chain policy vault for autonomous AI treasury agents.
//!
//! The agent may only *propose* actions. The contract decides whether to
//! auto-execute, queue for human approval, or reject — enforcing spending caps,
//! recipient allowlists and approval thresholds on-chain. This is the "trust
//! layer" that makes delegating funds to an AI agent safe: the agent never
//! holds authority over the funds, the contract does.

use odra::casper_types::U512;
use odra::prelude::*;

/// Length of the rolling spend window: 24 hours in milliseconds.
const WINDOW_MS: u64 = 86_400_000;

/// Custom user errors, surfaced on revert for easier debugging.
#[odra::odra_error]
pub enum Error {
    /// Caller is not the contract owner.
    NotOwner = 0,
    /// Caller is not the registered agent.
    NotAgent = 1,
    /// Contract is paused by the owner.
    Paused = 2,
    /// Action would exceed the daily or category spending cap.
    CapExceeded = 3,
    /// Cooldown between auto-executed actions is still active.
    CooldownActive = 4,
    /// Vault does not hold enough funds for the action.
    InsufficientFunds = 5,
    /// Referenced pending action does not exist.
    UnknownPending = 6,
    /// Pending action was already executed.
    AlreadyExecuted = 7,
    /// Pending action was already vetoed.
    AlreadyVetoed = 8,
    /// Provided amount must be greater than zero.
    ZeroAmount = 9,
}

/// The kind of action an agent can propose.
#[odra::odra_type]
pub enum ActionKind {
    /// A plain CSPR transfer to a recipient.
    NativeTransfer,
    /// An x402 micropayment settled on-chain (pay-per-use API/service).
    X402Payment,
}

/// The verdict returned by the policy engine for a proposed action.
#[odra::odra_type]
pub enum Decision {
    /// Action passed all policies and was executed immediately.
    AutoExecuted,
    /// Action requires human approval and was queued.
    Pending,
}

/// A queued action awaiting owner approval.
#[odra::odra_type]
pub struct PendingAction {
    pub id: u64,
    pub kind: ActionKind,
    pub recipient: Address,
    pub amount: U512,
    pub category: String,
    pub memo: String,
    pub proposed_by: Address,
    pub created_at: u64,
    pub executed: bool,
    pub vetoed: bool,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[odra::event]
pub struct Deposited {
    pub from: Address,
    pub amount: U512,
}

#[odra::event]
pub struct ActionProposed {
    pub id: u64,
    pub recipient: Address,
    pub amount: U512,
    pub category: String,
    pub memo: String,
}

#[odra::event]
pub struct ActionExecuted {
    pub id: u64,
    pub recipient: Address,
    pub amount: U512,
    pub category: String,
}

#[odra::event]
pub struct ActionQueued {
    pub id: u64,
    pub recipient: Address,
    pub amount: U512,
    pub reason: String,
}

#[odra::event]
pub struct ActionApproved {
    pub id: u64,
}

#[odra::event]
pub struct ActionVetoed {
    pub id: u64,
}

#[odra::event]
pub struct PolicyUpdated {
    pub field: String,
}

#[odra::event]
pub struct PausedSet {
    pub paused: bool,
}

#[odra::event]
pub struct Withdrawn {
    pub to: Address,
    pub amount: U512,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[odra::module(
    events = [
        Deposited, ActionProposed, ActionExecuted, ActionQueued,
        ActionApproved, ActionVetoed, PolicyUpdated, PausedSet, Withdrawn
    ],
    errors = Error
)]
pub struct TreasuryGuardian {
    owner: Var<Address>,
    agent: Var<Address>,
    paused: Var<bool>,

    // Policy
    daily_cap: Var<U512>,
    auto_threshold: Var<U512>,
    require_allowlist: Var<bool>,
    cooldown_ms: Var<u64>,
    allowlist: Mapping<Address, bool>,
    category_cap: Mapping<String, U512>,

    // Accounting (rolling 24h window)
    balance: Var<U512>,
    spent_today: Var<U512>,
    spent_today_by_cat: Mapping<String, U512>,
    window_start: Var<u64>,
    last_action_at: Var<u64>,
    has_acted: Var<bool>,

    // Pending queue
    next_id: Var<u64>,
    pending: Mapping<u64, PendingAction>,
}

#[odra::module]
impl TreasuryGuardian {
    /// Initializes the vault.
    ///
    /// * `agent` — the only account allowed to call `propose_action`.
    /// * `daily_cap` — max total auto-spend per rolling 24h window.
    /// * `auto_threshold` — actions at or below this auto-execute; above it
    ///   they are queued for owner approval.
    /// * `cooldown_ms` — minimum gap between two auto-executed actions.
    pub fn init(
        &mut self,
        agent: Address,
        daily_cap: U512,
        auto_threshold: U512,
        cooldown_ms: u64,
    ) {
        self.owner.set(self.env().caller());
        self.agent.set(agent);
        self.daily_cap.set(daily_cap);
        self.auto_threshold.set(auto_threshold);
        self.cooldown_ms.set(cooldown_ms);
        self.require_allowlist.set(true);
        self.paused.set(false);
        self.balance.set(U512::zero());
        self.spent_today.set(U512::zero());
        self.window_start.set(self.env().get_block_time());
        self.last_action_at.set(0);
        self.has_acted.set(false);
        self.next_id.set(0);
    }

    // ---- Funding ----------------------------------------------------------

    /// Deposits CSPR into the vault. Anyone may top up the treasury.
    #[odra(payable)]
    pub fn deposit(&mut self) {
        let amount = self.env().attached_value();
        self.balance.add(amount);
        self.env().emit_event(Deposited {
            from: self.env().caller(),
            amount,
        });
    }

    /// Withdraws CSPR from the vault back to an account. Owner only.
    pub fn withdraw(&mut self, amount: U512, to: Address) {
        self.assert_owner();
        if amount > self.balance.get_or_default() {
            self.env().revert(Error::InsufficientFunds);
        }
        self.balance.subtract(amount);
        self.env().transfer_tokens(&to, &amount);
        self.env().emit_event(Withdrawn { to, amount });
    }

    // ---- Agent: propose ---------------------------------------------------

    /// The agent proposes an action. The contract decides the outcome:
    /// auto-execute, queue for approval, or revert.
    pub fn propose_action(
        &mut self,
        kind: ActionKind,
        recipient: Address,
        amount: U512,
        category: String,
        memo: String,
    ) -> Decision {
        self.assert_not_paused();
        self.assert_agent();
        if amount.is_zero() {
            self.env().revert(Error::ZeroAmount);
        }
        self.roll_window_if_needed();

        // Unknown recipient + allowlist required => needs human approval.
        if self.require_allowlist.get_or_default() && !self.allowlist.get_or_default(&recipient) {
            return self.enqueue(kind, recipient, amount, category, memo, "not_allowlisted");
        }

        // Above auto threshold => needs human approval.
        if amount > self.auto_threshold.get_or_default() {
            return self.enqueue(kind, recipient, amount, category, memo, "above_threshold");
        }

        // Hard limits: caps and cooldown cause an on-chain revert.
        if self.would_exceed_caps(&category, amount) {
            self.env().revert(Error::CapExceeded);
        }
        if !self.cooldown_ok() {
            self.env().revert(Error::CooldownActive);
        }

        let id = self.bump_id();
        self.execute(id, kind, recipient, amount, &category, &memo);
        Decision::AutoExecuted
    }

    // ---- Owner: approval workflow ----------------------------------------

    /// Approves and executes a queued action. Owner override bypasses caps.
    pub fn approve(&mut self, id: u64) {
        self.assert_owner();
        let mut action = self.pending.get(&id).unwrap_or_revert_with(&self.env(), Error::UnknownPending);
        if action.executed {
            self.env().revert(Error::AlreadyExecuted);
        }
        if action.vetoed {
            self.env().revert(Error::AlreadyVetoed);
        }
        self.roll_window_if_needed();
        if action.amount > self.balance.get_or_default() {
            self.env().revert(Error::InsufficientFunds);
        }
        action.executed = true;
        self.pending.set(&id, action.clone());
        self.env().emit_event(ActionApproved { id });
        self.execute(id, action.kind, action.recipient, action.amount, &action.category, &action.memo);
    }

    /// Vetoes a queued action so it can never execute. Owner only.
    pub fn veto(&mut self, id: u64) {
        self.assert_owner();
        let mut action = self.pending.get(&id).unwrap_or_revert_with(&self.env(), Error::UnknownPending);
        if action.executed {
            self.env().revert(Error::AlreadyExecuted);
        }
        if action.vetoed {
            self.env().revert(Error::AlreadyVetoed);
        }
        action.vetoed = true;
        self.pending.set(&id, action);
        self.env().emit_event(ActionVetoed { id });
    }

    // ---- Owner: policy setters -------------------------------------------

    pub fn set_agent(&mut self, agent: Address) {
        self.assert_owner();
        self.agent.set(agent);
        self.emit_policy("agent");
    }

    pub fn set_daily_cap(&mut self, cap: U512) {
        self.assert_owner();
        self.daily_cap.set(cap);
        self.emit_policy("daily_cap");
    }

    pub fn set_auto_threshold(&mut self, threshold: U512) {
        self.assert_owner();
        self.auto_threshold.set(threshold);
        self.emit_policy("auto_threshold");
    }

    pub fn set_category_cap(&mut self, category: String, cap: U512) {
        self.assert_owner();
        self.category_cap.set(&category, cap);
        self.emit_policy("category_cap");
    }

    pub fn set_allowlist(&mut self, account: Address, allowed: bool) {
        self.assert_owner();
        self.allowlist.set(&account, allowed);
        self.emit_policy("allowlist");
    }

    pub fn set_require_allowlist(&mut self, required: bool) {
        self.assert_owner();
        self.require_allowlist.set(required);
        self.emit_policy("require_allowlist");
    }

    pub fn set_cooldown(&mut self, cooldown_ms: u64) {
        self.assert_owner();
        self.cooldown_ms.set(cooldown_ms);
        self.emit_policy("cooldown_ms");
    }

    pub fn pause(&mut self, paused: bool) {
        self.assert_owner();
        self.paused.set(paused);
        self.env().emit_event(PausedSet { paused });
    }

    // ---- Views ------------------------------------------------------------

    pub fn get_owner(&self) -> Address {
        self.owner.get().unwrap_or_revert_with(&self.env(), Error::NotOwner)
    }

    pub fn get_agent(&self) -> Address {
        self.agent.get().unwrap_or_revert_with(&self.env(), Error::NotAgent)
    }

    pub fn is_paused(&self) -> bool {
        self.paused.get_or_default()
    }

    pub fn get_balance(&self) -> U512 {
        self.balance.get_or_default()
    }

    pub fn get_daily_cap(&self) -> U512 {
        self.daily_cap.get_or_default()
    }

    pub fn get_auto_threshold(&self) -> U512 {
        self.auto_threshold.get_or_default()
    }

    pub fn get_cooldown(&self) -> u64 {
        self.cooldown_ms.get_or_default()
    }

    pub fn requires_allowlist(&self) -> bool {
        self.require_allowlist.get_or_default()
    }

    pub fn is_allowlisted(&self, account: Address) -> bool {
        self.allowlist.get_or_default(&account)
    }

    pub fn get_category_cap(&self, category: String) -> U512 {
        self.category_cap.get_or_default(&category)
    }

    pub fn spent_today(&self) -> U512 {
        self.spent_today.get_or_default()
    }

    /// Remaining auto-spend budget in the current 24h window.
    pub fn remaining_today(&self) -> U512 {
        let cap = self.daily_cap.get_or_default();
        let spent = self.spent_today.get_or_default();
        if spent >= cap {
            U512::zero()
        } else {
            cap - spent
        }
    }

    pub fn next_id(&self) -> u64 {
        self.next_id.get_or_default()
    }

    pub fn get_pending(&self, id: u64) -> PendingAction {
        self.pending.get(&id).unwrap_or_revert_with(&self.env(), Error::UnknownPending)
    }

    // ---- Internal helpers -------------------------------------------------

    fn assert_owner(&self) {
        if self.env().caller() != self.owner.get().unwrap_or_revert_with(&self.env(), Error::NotOwner) {
            self.env().revert(Error::NotOwner);
        }
    }

    fn assert_agent(&self) {
        if self.env().caller() != self.agent.get().unwrap_or_revert_with(&self.env(), Error::NotAgent) {
            self.env().revert(Error::NotAgent);
        }
    }

    fn assert_not_paused(&self) {
        if self.paused.get_or_default() {
            self.env().revert(Error::Paused);
        }
    }

    fn roll_window_if_needed(&mut self) {
        let now = self.env().get_block_time();
        let start = self.window_start.get_or_default();
        if now.saturating_sub(start) >= WINDOW_MS {
            self.spent_today.set(U512::zero());
            self.window_start.set(now);
        }
    }

    fn would_exceed_caps(&self, category: &str, amount: U512) -> bool {
        let new_total = self.spent_today.get_or_default() + amount;
        if new_total > self.daily_cap.get_or_default() {
            return true;
        }
        let cat_cap = self.category_cap.get_or_default(&category.to_string());
        if cat_cap > U512::zero() {
            let new_cat = self.spent_today_by_cat.get_or_default(&category.to_string()) + amount;
            if new_cat > cat_cap {
                return true;
            }
        }
        false
    }

    fn cooldown_ok(&self) -> bool {
        if !self.has_acted.get_or_default() {
            return true;
        }
        let now = self.env().get_block_time();
        let last = self.last_action_at.get_or_default();
        now.saturating_sub(last) >= self.cooldown_ms.get_or_default()
    }

    fn record_spend(&mut self, category: &str, amount: U512) {
        self.spent_today.add(amount);
        let key = category.to_string();
        let prev = self.spent_today_by_cat.get_or_default(&key);
        self.spent_today_by_cat.set(&key, prev + amount);
        self.last_action_at.set(self.env().get_block_time());
        self.has_acted.set(true);
    }

    fn execute(
        &mut self,
        id: u64,
        _kind: ActionKind,
        recipient: Address,
        amount: U512,
        category: &str,
        _memo: &str,
    ) {
        if amount > self.balance.get_or_default() {
            self.env().revert(Error::InsufficientFunds);
        }
        self.balance.subtract(amount);
        self.record_spend(category, amount);
        self.env().transfer_tokens(&recipient, &amount);
        self.env().emit_event(ActionExecuted {
            id,
            recipient,
            amount,
            category: category.to_string(),
        });
    }

    fn enqueue(
        &mut self,
        kind: ActionKind,
        recipient: Address,
        amount: U512,
        category: String,
        memo: String,
        reason: &str,
    ) -> Decision {
        let id = self.bump_id();
        let action = PendingAction {
            id,
            kind,
            recipient,
            amount,
            category: category.clone(),
            memo: memo.clone(),
            proposed_by: self.env().caller(),
            created_at: self.env().get_block_time(),
            executed: false,
            vetoed: false,
        };
        self.pending.set(&id, action);
        self.env().emit_event(ActionProposed {
            id,
            recipient,
            amount,
            category,
            memo,
        });
        self.env().emit_event(ActionQueued {
            id,
            recipient,
            amount,
            reason: reason.to_string(),
        });
        Decision::Pending
    }

    fn bump_id(&mut self) -> u64 {
        let id = self.next_id.get_or_default();
        self.next_id.set(id + 1);
        id
    }

    fn emit_policy(&self, field: &str) {
        self.env().emit_event(PolicyUpdated {
            field: field.to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActionExecuted, ActionKind, ActionQueued, ActionVetoed, Decision, Deposited, Error,
        PausedSet, TreasuryGuardian, TreasuryGuardianHostRef, TreasuryGuardianInitArgs,
    };
    use odra::casper_types::U512;
    use odra::host::{Deployer, HostEnv, HostRef};
    use odra::prelude::*;

    const DAILY_CAP: u64 = 1_000_000;
    const AUTO_THRESHOLD: u64 = 100_000;

    fn setup() -> (HostEnv, TreasuryGuardianHostRef) {
        setup_with(DAILY_CAP, AUTO_THRESHOLD, 0)
    }

    fn setup_with(
        daily_cap: u64,
        auto_threshold: u64,
        cooldown_ms: u64,
    ) -> (HostEnv, TreasuryGuardianHostRef) {
        let env = odra_test::env();
        // account(0) = owner (deployer), account(1) = agent.
        let agent = env.get_account(1);
        let contract = TreasuryGuardian::deploy(
            &env,
            TreasuryGuardianInitArgs {
                agent,
                daily_cap: U512::from(daily_cap),
                auto_threshold: U512::from(auto_threshold),
                cooldown_ms,
            },
        );
        (env, contract)
    }

    /// Funds the vault and allowlists `recipient`. Returns the recipient.
    fn fund_and_allowlist(env: &HostEnv, c: &mut TreasuryGuardianHostRef, amount: u64) -> Address {
        let owner = env.get_account(0);
        let recipient = env.get_account(2);
        env.set_caller(owner);
        c.with_tokens(U512::from(amount)).deposit();
        c.set_allowlist(recipient, true);
        recipient
    }

    #[test]
    fn init_sets_roles_and_policy() {
        let (env, c) = setup();
        assert_eq!(c.get_owner(), env.get_account(0));
        assert_eq!(c.get_agent(), env.get_account(1));
        assert_eq!(c.get_daily_cap(), U512::from(DAILY_CAP));
        assert_eq!(c.get_auto_threshold(), U512::from(AUTO_THRESHOLD));
        assert!(c.requires_allowlist());
        assert!(!c.is_paused());
    }

    #[test]
    fn deposit_increases_balance_and_emits() {
        let (env, mut c) = setup();
        let owner = env.get_account(0);
        env.set_caller(owner);
        c.with_tokens(U512::from(500_000u64)).deposit();
        assert_eq!(c.get_balance(), U512::from(500_000u64));
        assert!(env.emitted_event(
            &c,
            Deposited {
                from: owner,
                amount: U512::from(500_000u64)
            }
        ));
    }

    #[test]
    fn small_allowlisted_transfer_auto_executes() {
        let (env, mut c) = setup();
        let recipient = fund_and_allowlist(&env, &mut c, 500_000);

        env.set_caller(env.get_account(1)); // agent
        let decision = c.propose_action(
            ActionKind::NativeTransfer,
            recipient,
            U512::from(50_000u64),
            "infra".to_string(),
            "monthly server bill".to_string(),
        );

        assert_eq!(decision, Decision::AutoExecuted);
        assert_eq!(c.get_balance(), U512::from(450_000u64));
        assert_eq!(c.spent_today(), U512::from(50_000u64));
        assert_eq!(c.remaining_today(), U512::from(DAILY_CAP - 50_000));
        assert!(env.emitted_event(
            &c,
            ActionExecuted {
                id: 0,
                recipient,
                amount: U512::from(50_000u64),
                category: "infra".to_string()
            }
        ));
    }

    #[test]
    fn above_threshold_queues_then_owner_approves() {
        let (env, mut c) = setup();
        let recipient = fund_and_allowlist(&env, &mut c, 500_000);

        env.set_caller(env.get_account(1));
        let decision = c.propose_action(
            ActionKind::NativeTransfer,
            recipient,
            U512::from(200_000u64), // > auto_threshold
            "payroll".to_string(),
            "contractor".to_string(),
        );
        assert_eq!(decision, Decision::Pending);
        let pending = c.get_pending(0);
        assert!(!pending.executed);
        assert_eq!(pending.amount, U512::from(200_000u64));
        // Balance untouched until approved.
        assert_eq!(c.get_balance(), U512::from(500_000u64));

        // Owner approves -> executes.
        env.set_caller(env.get_account(0));
        c.approve(0);
        assert_eq!(c.get_balance(), U512::from(300_000u64));
        assert!(c.get_pending(0).executed);
    }

    #[test]
    fn non_allowlisted_recipient_is_queued() {
        let (env, mut c) = setup();
        fund_and_allowlist(&env, &mut c, 500_000);
        let stranger = env.get_account(3); // not allowlisted

        env.set_caller(env.get_account(1));
        let decision = c.propose_action(
            ActionKind::NativeTransfer,
            stranger,
            U512::from(10u64),
            "misc".to_string(),
            "unknown vendor".to_string(),
        );
        assert_eq!(decision, Decision::Pending);
        assert!(env.emitted_event(
            &c,
            ActionQueued {
                id: 0,
                recipient: stranger,
                amount: U512::from(10u64),
                reason: "not_allowlisted".to_string()
            }
        ));
    }

    #[test]
    fn exceeding_daily_cap_reverts() {
        // threshold high so it does not get queued; low daily cap.
        let (env, mut c) = setup_with(100_000, 1_000_000, 0);
        let recipient = fund_and_allowlist(&env, &mut c, 500_000);

        env.set_caller(env.get_account(1));
        let res = c.try_propose_action(
            ActionKind::NativeTransfer,
            recipient,
            U512::from(200_000u64), // <= threshold but > daily cap
            "infra".to_string(),
            "too big".to_string(),
        );
        assert_eq!(res, Err(Error::CapExceeded.into()));
    }

    #[test]
    fn category_cap_reverts() {
        let (env, mut c) = setup_with(1_000_000, 1_000_000, 0);
        let recipient = fund_and_allowlist(&env, &mut c, 500_000);
        env.set_caller(env.get_account(0));
        c.set_category_cap("data".to_string(), U512::from(10_000u64));

        env.set_caller(env.get_account(1));
        let res = c.try_propose_action(
            ActionKind::X402Payment,
            recipient,
            U512::from(20_000u64),
            "data".to_string(),
            "premium api".to_string(),
        );
        assert_eq!(res, Err(Error::CapExceeded.into()));
    }

    #[test]
    fn cooldown_blocks_then_allows_after_wait() {
        let (env, mut c) = setup_with(1_000_000, 1_000_000, 10_000);
        let recipient = fund_and_allowlist(&env, &mut c, 500_000);

        env.set_caller(env.get_account(1));
        c.propose_action(
            ActionKind::NativeTransfer,
            recipient,
            U512::from(1_000u64),
            "infra".to_string(),
            "first".to_string(),
        );
        // Second immediate action hits cooldown.
        let res = c.try_propose_action(
            ActionKind::NativeTransfer,
            recipient,
            U512::from(1_000u64),
            "infra".to_string(),
            "second".to_string(),
        );
        assert_eq!(res, Err(Error::CooldownActive.into()));

        // After cooldown elapses it succeeds.
        env.advance_block_time(10_000);
        let decision = c.propose_action(
            ActionKind::NativeTransfer,
            recipient,
            U512::from(1_000u64),
            "infra".to_string(),
            "third".to_string(),
        );
        assert_eq!(decision, Decision::AutoExecuted);
    }

    #[test]
    fn only_agent_can_propose() {
        let (env, mut c) = setup();
        let recipient = fund_and_allowlist(&env, &mut c, 500_000);
        env.set_caller(env.get_account(0)); // owner, not agent
        let res = c.try_propose_action(
            ActionKind::NativeTransfer,
            recipient,
            U512::from(10u64),
            "infra".to_string(),
            "x".to_string(),
        );
        assert_eq!(res, Err(Error::NotAgent.into()));
    }

    #[test]
    fn only_owner_can_approve_and_set_policy() {
        let (env, mut c) = setup();
        fund_and_allowlist(&env, &mut c, 500_000);
        // Queue a pending action as agent.
        let stranger = env.get_account(3);
        env.set_caller(env.get_account(1));
        c.propose_action(
            ActionKind::NativeTransfer,
            stranger,
            U512::from(10u64),
            "misc".to_string(),
            "x".to_string(),
        );
        // Non-owner cannot approve.
        env.set_caller(env.get_account(4));
        assert_eq!(c.try_approve(0), Err(Error::NotOwner.into()));
        assert_eq!(
            c.try_set_daily_cap(U512::from(1u64)),
            Err(Error::NotOwner.into())
        );
    }

    #[test]
    fn vetoed_action_cannot_be_approved() {
        let (env, mut c) = setup();
        fund_and_allowlist(&env, &mut c, 500_000);
        let stranger = env.get_account(3);
        env.set_caller(env.get_account(1));
        c.propose_action(
            ActionKind::NativeTransfer,
            stranger,
            U512::from(10u64),
            "misc".to_string(),
            "x".to_string(),
        );
        env.set_caller(env.get_account(0));
        c.veto(0);
        assert!(env.emitted_event(&c, ActionVetoed { id: 0 }));
        assert_eq!(c.try_approve(0), Err(Error::AlreadyVetoed.into()));
    }

    #[test]
    fn paused_blocks_proposals() {
        let (env, mut c) = setup();
        let recipient = fund_and_allowlist(&env, &mut c, 500_000);
        env.set_caller(env.get_account(0));
        c.pause(true);
        assert!(env.emitted_event(&c, PausedSet { paused: true }));
        env.set_caller(env.get_account(1));
        let res = c.try_propose_action(
            ActionKind::NativeTransfer,
            recipient,
            U512::from(10u64),
            "infra".to_string(),
            "x".to_string(),
        );
        assert_eq!(res, Err(Error::Paused.into()));
    }

    #[test]
    fn withdraw_only_owner_and_respects_balance() {
        let (env, mut c) = setup();
        let owner = env.get_account(0);
        env.set_caller(owner);
        c.with_tokens(U512::from(100_000u64)).deposit();

        // Non-owner cannot withdraw.
        env.set_caller(env.get_account(5));
        assert_eq!(
            c.try_withdraw(U512::from(1u64), env.get_account(5)),
            Err(Error::NotOwner.into())
        );

        // Owner cannot over-withdraw.
        env.set_caller(owner);
        assert_eq!(
            c.try_withdraw(U512::from(200_000u64), owner),
            Err(Error::InsufficientFunds.into())
        );

        // Valid withdraw works.
        c.withdraw(U512::from(40_000u64), owner);
        assert_eq!(c.get_balance(), U512::from(60_000u64));
    }

    #[test]
    fn zero_amount_reverts() {
        let (env, mut c) = setup();
        let recipient = fund_and_allowlist(&env, &mut c, 500_000);
        env.set_caller(env.get_account(1));
        let res = c.try_propose_action(
            ActionKind::NativeTransfer,
            recipient,
            U512::zero(),
            "infra".to_string(),
            "x".to_string(),
        );
        assert_eq!(res, Err(Error::ZeroAmount.into()));
    }
}
