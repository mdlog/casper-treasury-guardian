//! Deploy the Treasury Guardian vault to Casper Testnet via Odra's livenet env.
//!
//! Prerequisites (see ../.env.example and ../DEPLOY.md):
//!   * A funded testnet secret key (ODRA_CASPER_LIVENET_SECRET_KEY_PATH)
//!   * A reachable node RPC (ODRA_CASPER_LIVENET_NODE_ADDRESS) — when using
//!     CSPR.cloud, run `node cspr_proxy.js` and point this at the local proxy.
//!   * ODRA_CASPER_LIVENET_CHAIN_NAME=casper-test
//!
//! Usage:
//!   cargo run --bin deploy_testnet --features=livenet
//!
//! Optional env knobs (CSPR; converted to motes, 1 CSPR = 1e9 motes):
//!   TG_DAILY_CAP_CSPR        default 1000
//!   TG_AUTO_THRESHOLD_CSPR   default 100
//!   TG_COOLDOWN_MS           default 0
//!   TG_AGENT_PUBLIC_KEY      hex public key of the agent account; defaults to
//!                            the deployer (so a single key can demo end-to-end)

use odra::casper_types::{AsymmetricType, PublicKey, U512};
use odra::host::Deployer;
use odra::prelude::{Address, Addressable};
use treasury_guardian::treasury_guardian::{TreasuryGuardian, TreasuryGuardianInitArgs};

const MOTES_PER_CSPR: u64 = 1_000_000_000;

fn cspr_env(key: &str, default_cspr: u64) -> U512 {
    let cspr = std::env::var(key)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default_cspr);
    U512::from(cspr) * U512::from(MOTES_PER_CSPR)
}

fn main() {
    let env = odra_casper_livenet_env::env();
    let deployer = env.caller();

    // The agent defaults to the deployer for a single-key demo. In production
    // set TG_AGENT_PUBLIC_KEY to a dedicated agent account.
    let agent = match std::env::var("TG_AGENT_PUBLIC_KEY") {
        Ok(hex) if !hex.trim().is_empty() => {
            let pk = PublicKey::from_hex(hex.trim()).expect("invalid TG_AGENT_PUBLIC_KEY hex");
            Address::from(pk)
        }
        _ => deployer,
    };

    let daily_cap = cspr_env("TG_DAILY_CAP_CSPR", 1000);
    let auto_threshold = cspr_env("TG_AUTO_THRESHOLD_CSPR", 100);
    let cooldown_ms: u64 = std::env::var("TG_COOLDOWN_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    println!("Deploying Treasury Guardian to Casper Testnet");
    println!("  deployer/owner : {:?}", deployer);
    println!("  agent          : {:?}", agent);
    println!("  daily_cap      : {} motes", daily_cap);
    println!("  auto_threshold : {} motes", auto_threshold);
    println!("  cooldown_ms    : {}", cooldown_ms);

    // Deployment of the vault is the heaviest call.
    env.set_gas(300_000_000_000u64); // 300 CSPR
    let contract = TreasuryGuardian::deploy(
        &env,
        TreasuryGuardianInitArgs {
            agent,
            daily_cap,
            auto_threshold,
            cooldown_ms,
        },
    );

    println!();
    println!("Contract deployed.");
    println!("  address : {:?}", contract.address());
    println!("Inspect it on https://testnet.cspr.live/");
}
