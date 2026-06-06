/**
 * Treasury reasoning. Turns the (paid-for) market intelligence into a concrete
 * action proposal. Rule-based by default so the demo runs without any LLM key;
 * the same `decideAction` signature is where an LLM planner would slot in.
 */
import { config } from "./config.js";

export interface MarketIntel {
  asset: string;
  price: number;
  volatility24h: number;
  treasuryRiskScore: number; // 0 (safe) .. 100 (risky)
  recommendation: "reduce_exposure" | "accumulate" | "hold";
  runwayAdvice: string;
}

export interface PlannedAction {
  kind: "native";
  recipientPubKeyHex: string;
  cspr: number;
  category: string;
  memo: string;
  rationale: string;
}

/**
 * Map risk to a rebalancing transfer into the reserve account. The on-chain
 * vault still has the final say (caps / allowlist / threshold) — this only
 * *proposes*.
 */
export function decideAction(intel: MarketIntel): PlannedAction | null {
  const risk = intel.treasuryRiskScore;

  if (intel.recommendation !== "reduce_exposure" || risk <= 65) {
    return null; // nothing to do; hold.
  }

  // Size the de-risking transfer with risk: higher risk → move more, capped
  // small so it tends to auto-execute and stays well within daily caps.
  const cspr = Math.min(50, Math.round(5 + (risk - 65) * 1.2));

  return {
    kind: "native",
    recipientPubKeyHex: config.reserveAccount,
    cspr,
    category: "rebalance",
    memo: `derisk: risk=${risk} vol=${intel.volatility24h} price=${intel.price}`,
    rationale:
      `Risk score ${risk} > 65 with recommendation '${intel.recommendation}'. ` +
      `Move ${cspr} CSPR to the reserve to reduce exposure.`,
  };
}
