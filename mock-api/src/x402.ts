/**
 * Minimal x402 ("HTTP 402 Payment Required") protocol types for a Casper-settled
 * exact-amount scheme. Mirrors the shape of the Coinbase x402 spec
 * (https://github.com/coinbase/x402) adapted to native CSPR settlement.
 */

export const X402_VERSION = 1;
export const CASPER_TESTNET = "casper-testnet";

/** What a server demands before serving a paid resource. */
export interface PaymentRequirements {
  /** Settlement scheme. We implement "exact" — pay an exact amount. */
  scheme: "exact";
  /** Settlement network. */
  network: typeof CASPER_TESTNET;
  /** Amount required, denominated in motes (1 CSPR = 1e9 motes), as a string. */
  maxAmountRequired: string;
  /** The resource (path) being purchased. */
  resource: string;
  /** Human-readable description of the resource. */
  description: string;
  /** MIME type of the resource that will be returned on success. */
  mimeType: string;
  /** Account that must receive the payment (the provider). Public key hex. */
  payTo: string;
  /** Asset symbol. Native CSPR for this demo. */
  asset: string;
  /** Max seconds the client has to settle before the offer expires. */
  maxTimeoutSeconds: number;
}

/** Body returned alongside an HTTP 402 response. */
export interface PaymentRequiredResponse {
  x402Version: number;
  accepts: PaymentRequirements[];
  error?: string;
}

/** Scheme-specific settlement evidence for native CSPR on Casper. */
export interface CasperExactPayload {
  /** Hash of the on-chain transaction that settled the payment. */
  txHash: string;
  /** Payer account (public key hex). */
  from: string;
  /** Payee account (public key hex) — must match `payTo`. */
  to: string;
  /** Amount paid in motes, as a string. */
  amount: string;
}

/** Value carried in the `X-PAYMENT` request header (base64-encoded JSON). */
export interface PaymentPayload {
  x402Version: number;
  scheme: "exact";
  network: typeof CASPER_TESTNET;
  payload: CasperExactPayload;
}

/** Value returned in the `X-PAYMENT-RESPONSE` header (base64-encoded JSON). */
export interface SettlementResponse {
  success: boolean;
  txHash: string;
  network: string;
}

export function encodeHeader(value: unknown): string {
  return Buffer.from(JSON.stringify(value), "utf8").toString("base64");
}

export function decodePaymentHeader(header: string): PaymentPayload {
  const json = Buffer.from(header, "base64").toString("utf8");
  return JSON.parse(json) as PaymentPayload;
}
