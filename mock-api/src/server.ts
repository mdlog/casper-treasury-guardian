/**
 * Mock "premium" API gated by the x402 payment protocol.
 *
 * GET /api/market-intel
 *   - Without an `X-PAYMENT` header → 402 Payment Required + PaymentRequirements.
 *   - With a valid `X-PAYMENT` header → 200 + the premium payload, and an
 *     `X-PAYMENT-RESPONSE` header acknowledging settlement.
 *
 * This is what a Treasury Guardian agent pays for, per call, through the vault.
 */
import cors from "cors";
import express, { type Request, type Response } from "express";
import {
  CASPER_TESTNET,
  decodePaymentHeader,
  encodeHeader,
  X402_VERSION,
  type PaymentRequiredResponse,
  type PaymentRequirements,
  type SettlementResponse,
} from "./x402.js";

const PORT = Number(process.env.PORT ?? 4021);
// Provider account that must be paid (public key hex). Overridable via env so it
// can be added to the vault allowlist for the demo.
const PROVIDER_ACCOUNT =
  process.env.PROVIDER_ACCOUNT ??
  "0202531fe6068134503d2723133227c867ac8fa6c83c537e9a44c3c5bdbdcb1fe7a0";
// Price of one call, in motes (default 5 CSPR).
const PRICE_MOTES = process.env.PRICE_MOTES ?? "5000000000";
const RESOURCE = "/api/market-intel";

const app = express();
app.use(cors({ exposedHeaders: ["X-PAYMENT-RESPONSE"] }));
app.use(express.json());

function requirements(): PaymentRequirements {
  return {
    scheme: "exact",
    network: CASPER_TESTNET,
    maxAmountRequired: PRICE_MOTES,
    resource: RESOURCE,
    description: "Premium market intelligence: treasury risk score & price feed",
    mimeType: "application/json",
    payTo: PROVIDER_ACCOUNT,
    asset: "CSPR",
    maxTimeoutSeconds: 120,
  };
}

/** The actual premium product, served only after payment. */
function premiumPayload() {
  // Deterministic-ish synthetic "market intelligence" for the demo.
  const now = Date.now();
  const cosphase = Math.cos(now / 6.0e8);
  const price = Number((0.012 + 0.0008 * cosphase).toFixed(6));
  const volatility = Number((0.35 + 0.1 * Math.abs(cosphase)).toFixed(3));
  const riskScore = Math.round(40 + 30 * (1 - cosphase));
  return {
    generatedAt: new Date(now).toISOString(),
    asset: "CSPR/USD",
    price,
    volatility24h: volatility,
    treasuryRiskScore: riskScore, // 0 (safe) .. 100 (risky)
    recommendation:
      riskScore > 65 ? "reduce_exposure" : riskScore < 45 ? "accumulate" : "hold",
    runwayAdvice:
      "Maintain >= 6 months stablecoin runway; rebalance if risk score > 65.",
  };
}

app.get("/", (_req: Request, res: Response) => {
  res.json({
    name: "Treasury Guardian — x402 premium API (mock)",
    endpoints: { premium: RESOURCE },
    priceMotes: PRICE_MOTES,
    payTo: PROVIDER_ACCOUNT,
    network: CASPER_TESTNET,
  });
});

app.get("/health", (_req: Request, res: Response) => {
  res.json({ ok: true });
});

app.get(RESOURCE, (req: Request, res: Response) => {
  const header = req.header("X-PAYMENT");

  // No payment yet → respond 402 with what we require.
  if (!header) {
    const body: PaymentRequiredResponse = {
      x402Version: X402_VERSION,
      accepts: [requirements()],
      error: "X-PAYMENT header is required",
    };
    res.status(402).json(body);
    return;
  }

  // Verify the supplied settlement evidence.
  try {
    const payment = decodePaymentHeader(header);
    const need = requirements();

    const problems: string[] = [];
    if (payment.x402Version !== X402_VERSION) problems.push("bad x402Version");
    if (payment.scheme !== need.scheme) problems.push("bad scheme");
    if (payment.network !== need.network) problems.push("bad network");
    if (payment.payload.to !== need.payTo) problems.push("wrong payTo");
    if (BigInt(payment.payload.amount) < BigInt(need.maxAmountRequired))
      problems.push("insufficient amount");
    if (!payment.payload.txHash || payment.payload.txHash.length < 8)
      problems.push("missing txHash");

    if (problems.length > 0) {
      const body: PaymentRequiredResponse = {
        x402Version: X402_VERSION,
        accepts: [need],
        error: `invalid payment: ${problems.join(", ")}`,
      };
      res.status(402).json(body);
      return;
    }

    // In production the facilitator/server confirms the transfer on-chain via
    // the node RPC before serving. For this prototype we accept well-formed,
    // sufficient settlement evidence produced by the vault execution.
    const settlement: SettlementResponse = {
      success: true,
      txHash: payment.payload.txHash,
      network: payment.network,
    };
    res.setHeader("X-PAYMENT-RESPONSE", encodeHeader(settlement));
    res.json({ paid: true, data: premiumPayload() });
  } catch (err) {
    res.status(400).json({ error: `malformed X-PAYMENT header: ${String(err)}` });
  }
});

app.listen(PORT, () => {
  console.log(`[mock-api] x402 premium API listening on http://localhost:${PORT}`);
  console.log(`[mock-api] premium resource: ${RESOURCE} (price ${PRICE_MOTES} motes)`);
  console.log(`[mock-api] payTo: ${PROVIDER_ACCOUNT}`);
});
