/**
 * x402 client. Implements the agent half of the HTTP-402 handshake against the
 * mock premium API, settling payment *through the vault* so policy is enforced
 * on-chain. See ../../mock-api/src/x402.ts for the matching server types.
 */
import { config } from "./config.js";
import { propose, type ProposeResult } from "./onchain.js";

const X402_VERSION = 1;
const MOTES_PER_CSPR = 1_000_000_000;

interface PaymentRequirements {
  scheme: "exact";
  network: string;
  maxAmountRequired: string;
  resource: string;
  description: string;
  mimeType: string;
  payTo: string;
  asset: string;
  maxTimeoutSeconds: number;
}

interface PaymentRequiredResponse {
  x402Version: number;
  accepts: PaymentRequirements[];
  error?: string;
}

export interface X402Result {
  /** The premium data returned after successful payment. */
  data: unknown;
  /** The on-chain settlement decision from the vault. */
  settlement: ProposeResult | { decision: "Simulated"; txHash: string };
  requirements: PaymentRequirements;
}

function buildPaymentHeader(req: PaymentRequirements, from: string, txHash: string): string {
  const payload = {
    x402Version: X402_VERSION,
    scheme: "exact" as const,
    network: req.network,
    payload: {
      txHash,
      from,
      to: req.payTo,
      amount: req.maxAmountRequired,
    },
  };
  return Buffer.from(JSON.stringify(payload), "utf8").toString("base64");
}

/**
 * Fetch an x402-gated resource:
 *   1. GET it → expect 402 + PaymentRequirements.
 *   2. Settle the exact amount through the vault (propose X402Payment).
 *      - AutoExecuted → we have an on-chain tx hash to prove payment.
 *      - Pending      → human approval required; we cannot complete this call.
 *   3. Re-GET with the X-PAYMENT header → expect 200 + data.
 */
export async function payAndFetch(): Promise<X402Result> {
  const url = `${config.mockApiUrl}${config.resourcePath}`;

  const first = await fetch(url);
  if (first.status !== 402) {
    // Either free or already paid — return whatever we got.
    const data = await first.json();
    const req = (data as PaymentRequiredResponse).accepts?.[0];
    return {
      data,
      settlement: { decision: "Simulated", txHash: "none" },
      requirements: req,
    };
  }

  const body = (await first.json()) as PaymentRequiredResponse;
  const req = body.accepts[0];
  const cspr = Number(BigInt(req.maxAmountRequired)) / MOTES_PER_CSPR;
  console.log(
    `[x402] 402 received: pay ${cspr} CSPR to ${req.payTo.slice(0, 12)}… for ${req.resource}`
  );

  let from = req.payTo;
  let txHash: string;
  let settlement: X402Result["settlement"];

  if (config.dryRun) {
    // Offline demo: simulate settlement so the HTTP handshake can complete.
    txHash = `simulated${Date.now().toString(16)}`.padEnd(16, "0");
    settlement = { decision: "Simulated", txHash };
    console.log(`[x402] DRY-RUN: simulating settlement tx ${txHash}`);
  } else {
    // Real settlement through the vault. Category "data" keeps x402 spend
    // bucketed so the owner can cap API spending independently.
    const result = await propose("x402", req.payTo, cspr, "data", `x402:${req.resource}`);
    settlement = result;
    if (result.decision !== "AutoExecuted" || !result.txHash) {
      throw new Error(
        `x402 payment not settled on-chain (decision=${result.decision}). ` +
          `It likely needs owner approval or exceeded a cap.`
      );
    }
    txHash = result.txHash;
    from = result.txHash; // payer identity is implied by the on-chain tx
    console.log(`[x402] settled on-chain: tx ${txHash}`);
  }

  const paymentHeader = buildPaymentHeader(req, from, txHash);
  const second = await fetch(url, { headers: { "X-PAYMENT": paymentHeader } });
  if (!second.ok) {
    const errText = await second.text();
    throw new Error(`x402 retry failed (${second.status}): ${errText}`);
  }
  const paid = await second.json();
  return { data: (paid as { data: unknown }).data ?? paid, settlement, requirements: req };
}
