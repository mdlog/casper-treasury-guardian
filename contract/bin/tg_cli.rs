//! Treasury Guardian on-chain gateway CLI (livenet).
//!
//! A single, robust entry point for every on-chain operation, using Odra's
//! livenet bindings so runtime-argument encoding is always correct. The
//! TypeScript agent and the dashboard backend shell out to this binary instead
//! of re-implementing Odra serialization in JS.
//!
//! Config via env / .env (loaded by odra-casper-livenet-env):
//!   ODRA_CASPER_LIVENET_SECRET_KEY_PATH, ODRA_CASPER_LIVENET_NODE_ADDRESS,
//!   ODRA_CASPER_LIVENET_CHAIN_NAME, and TG_CONTRACT_ADDRESS (package hash of
//!   the deployed vault, e.g. "contract-package-....." or "hash-.....").
//!
//! Usage:
//!   cargo run --bin tg_cli --features=livenet -- <command> [args]
//!
//! Commands:
//!   state                                  Print vault state as JSON
//!   pending <id>                           Print one queued action as JSON
//!   deposit <cspr>                         Fund the vault
//!   withdraw <cspr> <to_pubkey_hex>        Withdraw to an account
//!   allowlist <pubkey_hex> <true|false>    Toggle a recipient on the allowlist
//!   set-daily-cap <cspr>                   Update the rolling 24h cap
//!   set-auto-threshold <cspr>             Update the auto-execute threshold
//!   set-cooldown <ms>                      Update the inter-action cooldown
//!   set-category-cap <name> <cspr>         Update a per-category cap
//!   require-allowlist <true|false>         Toggle allowlist enforcement
//!   pause <true|false>                     Pause/unpause the vault
//!   propose <native|x402> <recipient_pubkey_hex> <cspr> <category> <memo>
//!                                          Agent proposes an action
//!   approve <id>                           Owner approves a queued action
//!   veto <id>                              Owner vetoes a queued action

use odra::casper_types::{AsymmetricType, PublicKey, U512};
use odra::host::{HostEnv, HostRef, HostRefLoader};
use odra::prelude::Address;
use std::str::FromStr;
use treasury_guardian::treasury_guardian::{
    ActionKind, TreasuryGuardian, TreasuryGuardianHostRef,
};

const MOTES_PER_CSPR: u64 = 1_000_000_000;

fn cspr_to_motes(s: &str) -> U512 {
    let cspr: f64 = s.parse().expect("amount must be a number (CSPR)");
    let motes = (cspr * MOTES_PER_CSPR as f64).round() as u128;
    U512::from(motes)
}

fn motes_to_cspr(m: U512) -> String {
    // Best-effort human display; exact value also printed in motes.
    let motes = m.as_u128();
    format!("{}", motes as f64 / MOTES_PER_CSPR as f64)
}

fn pubkey_to_address(hex: &str) -> Address {
    let pk = PublicKey::from_hex(hex).expect("invalid public key hex");
    Address::from(pk)
}

fn load_contract(env: &HostEnv) -> TreasuryGuardianHostRef {
    let addr = std::env::var("TG_CONTRACT_ADDRESS")
        .expect("set TG_CONTRACT_ADDRESS to the deployed vault package hash");
    let address = Address::from_str(addr.trim()).expect("invalid TG_CONTRACT_ADDRESS");
    TreasuryGuardian::load(env, address)
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Emit one line of machine-readable JSON, marker-prefixed so callers can find
/// it amid livenet's human-oriented deploy logging.
fn emit(json: String) {
    println!("TG_JSON {}", json);
}

fn print_state(c: &TreasuryGuardianHostRef) {
    let balance = c.get_balance();
    let daily_cap = c.get_daily_cap();
    let auto = c.get_auto_threshold();
    emit(format!(
        concat!(
            "{{\"owner\":\"{:?}\",\"agent\":\"{:?}\",\"paused\":{},",
            "\"requireAllowlist\":{},\"balanceMotes\":\"{}\",\"balanceCspr\":{},",
            "\"dailyCapMotes\":\"{}\",\"dailyCapCspr\":{},",
            "\"autoThresholdMotes\":\"{}\",\"autoThresholdCspr\":{},",
            "\"cooldownMs\":{},\"spentTodayMotes\":\"{}\",",
            "\"remainingTodayMotes\":\"{}\",\"nextId\":{}}}"
        ),
        c.get_owner(),
        c.get_agent(),
        c.is_paused(),
        c.requires_allowlist(),
        balance,
        motes_to_cspr(balance),
        daily_cap,
        motes_to_cspr(daily_cap),
        auto,
        motes_to_cspr(auto),
        c.get_cooldown(),
        c.spent_today(),
        c.remaining_today(),
        c.next_id(),
    ));
}

fn print_pending(c: &TreasuryGuardianHostRef, id: u64) {
    let a = c.get_pending(id);
    let kind = match a.kind {
        ActionKind::NativeTransfer => "NativeTransfer",
        ActionKind::X402Payment => "X402Payment",
    };
    emit(format!(
        concat!(
            "{{\"id\":{},\"kind\":\"{}\",\"recipient\":\"{:?}\",",
            "\"amountMotes\":\"{}\",\"amountCspr\":{},\"category\":\"{}\",",
            "\"memo\":\"{}\",\"createdAt\":{},\"executed\":{},\"vetoed\":{}}}"
        ),
        a.id,
        kind,
        a.recipient,
        a.amount,
        motes_to_cspr(a.amount),
        json_escape(&a.category),
        json_escape(&a.memo),
        a.created_at,
        a.executed,
        a.vetoed,
    ));
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: tg_cli <command> [args] (see file header for commands)");
        std::process::exit(2);
    }

    let env = odra_casper_livenet_env::env();
    let mut c = load_contract(&env);

    let cmd = args[0].as_str();
    match cmd {
        "state" => print_state(&c),
        "pending" => {
            let id: u64 = args[1].parse().expect("id must be a number");
            print_pending(&c, id);
        }
        "deposit" => {
            let motes = cspr_to_motes(&args[1]);
            env.set_gas(10_000_000_000u64);
            c.with_tokens(motes).deposit();
            emit(format!("{{\"ok\":true,\"action\":\"deposit\",\"motes\":\"{}\"}}", motes));
        }
        "withdraw" => {
            let motes = cspr_to_motes(&args[1]);
            let to = pubkey_to_address(&args[2]);
            env.set_gas(10_000_000_000u64);
            c.withdraw(motes, to);
            emit(format!("{{\"ok\":true,\"action\":\"withdraw\",\"motes\":\"{}\"}}", motes));
        }
        "allowlist" => {
            let acct = pubkey_to_address(&args[1]);
            let allowed: bool = args[2].parse().expect("expected true|false");
            env.set_gas(5_000_000_000u64);
            c.set_allowlist(acct, allowed);
            emit(format!("{{\"ok\":true,\"action\":\"allowlist\",\"allowed\":{}}}", allowed));
        }
        "set-daily-cap" => {
            let motes = cspr_to_motes(&args[1]);
            env.set_gas(5_000_000_000u64);
            c.set_daily_cap(motes);
            emit(format!("{{\"ok\":true,\"action\":\"set-daily-cap\",\"motes\":\"{}\"}}", motes));
        }
        "set-auto-threshold" => {
            let motes = cspr_to_motes(&args[1]);
            env.set_gas(5_000_000_000u64);
            c.set_auto_threshold(motes);
            emit(format!(
                "{{\"ok\":true,\"action\":\"set-auto-threshold\",\"motes\":\"{}\"}}",
                motes
            ));
        }
        "set-cooldown" => {
            let ms: u64 = args[1].parse().expect("ms must be a number");
            env.set_gas(5_000_000_000u64);
            c.set_cooldown(ms);
            emit(format!("{{\"ok\":true,\"action\":\"set-cooldown\",\"ms\":{}}}", ms));
        }
        "set-category-cap" => {
            let name = args[1].clone();
            let motes = cspr_to_motes(&args[2]);
            env.set_gas(5_000_000_000u64);
            c.set_category_cap(name.clone(), motes);
            emit(format!(
                "{{\"ok\":true,\"action\":\"set-category-cap\",\"category\":\"{}\",\"motes\":\"{}\"}}",
                json_escape(&name), motes
            ));
        }
        "require-allowlist" => {
            let required: bool = args[1].parse().expect("expected true|false");
            env.set_gas(5_000_000_000u64);
            c.set_require_allowlist(required);
            emit(format!(
                "{{\"ok\":true,\"action\":\"require-allowlist\",\"required\":{}}}",
                required
            ));
        }
        "pause" => {
            let paused: bool = args[1].parse().expect("expected true|false");
            env.set_gas(5_000_000_000u64);
            c.pause(paused);
            emit(format!("{{\"ok\":true,\"action\":\"pause\",\"paused\":{}}}", paused));
        }
        "propose" => {
            let kind = match args[1].as_str() {
                "native" => ActionKind::NativeTransfer,
                "x402" => ActionKind::X402Payment,
                other => panic!("unknown kind '{}', expected native|x402", other),
            };
            let recipient = pubkey_to_address(&args[2]);
            let motes = cspr_to_motes(&args[3]);
            let category = args[4].clone();
            let memo = args.get(5).cloned().unwrap_or_default();
            env.set_gas(15_000_000_000u64);
            let decision = c.propose_action(kind, recipient, motes, category.clone(), memo.clone());
            emit(format!(
                "{{\"ok\":true,\"action\":\"propose\",\"decision\":\"{:?}\",\"amountMotes\":\"{}\",\"category\":\"{}\"}}",
                decision, motes, json_escape(&category)
            ));
        }
        "approve" => {
            let id: u64 = args[1].parse().expect("id must be a number");
            env.set_gas(15_000_000_000u64);
            c.approve(id);
            emit(format!("{{\"ok\":true,\"action\":\"approve\",\"id\":{}}}", id));
        }
        "veto" => {
            let id: u64 = args[1].parse().expect("id must be a number");
            env.set_gas(5_000_000_000u64);
            c.veto(id);
            emit(format!("{{\"ok\":true,\"action\":\"veto\",\"id\":{}}}", id));
        }
        other => {
            eprintln!("unknown command: {}", other);
            std::process::exit(2);
        }
    }
}
