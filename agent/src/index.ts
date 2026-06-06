/**
 * Treasury Guardian agent — main loop.
 *
 * Each cycle:
 *   1. Pay for premium market intelligence via x402 (settled through the vault).
 *   2. Reason about treasury risk.
 *   3. Propose a treasury action to the vault, which ENFORCES policy on-chain:
 *      auto-execute (small/allowlisted), queue for human approval, or revert.
 *
 * The agent only ever *proposes*. It never holds the treasury keys; the vault
 * contract is the trust boundary. Run with --once for a single cycle, and
 * --dry-run to exercise reasoning + the x402 handshake without touching chain.
 */
import { assertContractDir, config } from "./config.js";
import { getState } from "./onchain.js";
import { decideAction, type MarketIntel } from "./reason.js";
import { propose } from "./onchain.js";
import { payAndFetch } from "./x402.js";

function log(section: string, msg: string): void {
  console.log(`\n=== ${section} ===\n${msg}`);
}

async function cycle(): Promise<void> {
  // 1) Show vault state (skipped in dry-run where there may be no contract).
  if (!config.dryRun) {
    const s = await getState();
    log(
      "VAULT STATE",
      [
        `balance:           ${s.balanceCspr} CSPR`,
        `daily cap:         ${s.dailyCapCspr} CSPR (remaining ${
          Number(BigInt(s.remainingTodayMotes)) / 1e9
        } CSPR)`,
        `auto threshold:    ${s.autoThresholdCspr} CSPR`,
        `require allowlist: ${s.requireAllowlist}`,
        `paused:            ${s.paused}`,
      ].join("\n")
    );
    if (s.paused) {
      log("HALT", "Vault is paused; agent stands down this cycle.");
      return;
    }
  }

  // 2) Buy market intelligence via x402.
  const { data, settlement, requirements } = await payAndFetch();
  const intel = data as MarketIntel;
  log(
    "x402 PURCHASE",
    [
      `resource:    ${requirements.resource}`,
      `price:       ${Number(BigInt(requirements.maxAmountRequired)) / 1e9} CSPR`,
      `settlement:  ${settlement.decision} (tx ${settlement.txHash ?? "n/a"})`,
      `intel:       risk=${intel.treasuryRiskScore} rec=${intel.recommendation} price=${intel.price}`,
    ].join("\n")
  );

  // 3) Decide and propose.
  const action = decideAction(intel);
  if (!action) {
    log("DECISION", "Hold — no rebalance needed this cycle.");
    return;
  }
  log("DECISION", action.rationale);

  if (config.dryRun) {
    log(
      "PROPOSE (dry-run)",
      `Would propose ${action.cspr} CSPR to ${action.recipientPubKeyHex.slice(
        0,
        12
      )}… category=${action.category}`
    );
    return;
  }

  const result = await propose(
    action.kind,
    action.recipientPubKeyHex,
    action.cspr,
    action.category,
    action.memo
  );
  log(
    "VAULT ENFORCEMENT",
    [
      `proposed:  ${action.cspr} CSPR → ${action.recipientPubKeyHex.slice(0, 12)}…`,
      `decision:  ${result.decision}`,
      result.decision === "AutoExecuted"
        ? `executed on-chain: tx ${result.txHash ?? "n/a"}`
        : `queued for owner approval (open the dashboard to approve/veto)`,
    ].join("\n")
  );
}

async function main(): Promise<void> {
  assertContractDir();
  console.log(
    `Treasury Guardian agent starting (dryRun=${config.dryRun}, once=${config.once})`
  );
  console.log(`  mock API: ${config.mockApiUrl}${config.resourcePath}`);
  console.log(`  contract: ${config.contractDir}`);

  if (config.once) {
    await cycle();
    return;
  }

  // Continuous loop.
  // eslint-disable-next-line no-constant-condition
  while (true) {
    try {
      await cycle();
    } catch (err) {
      console.error(`[agent] cycle error: ${String(err)}`);
    }
    await new Promise((r) => setTimeout(r, config.intervalMs));
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
