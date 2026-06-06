/**
 * Thin wrapper around the Rust `tg_cli` livenet binary. We delegate every
 * on-chain operation to it so Odra handles runtime-argument encoding and
 * signing — the agent never re-implements Casper serialization in JS, and it
 * never holds custody of the treasury (the vault contract enforces policy).
 */
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { config } from "./config.js";

const execFileAsync = promisify(execFile);

export type Decision = "AutoExecuted" | "Pending";

export interface VaultState {
  owner: string;
  agent: string;
  paused: boolean;
  requireAllowlist: boolean;
  balanceMotes: string;
  balanceCspr: number;
  dailyCapMotes: string;
  dailyCapCspr: number;
  autoThresholdMotes: string;
  autoThresholdCspr: number;
  cooldownMs: number;
  spentTodayMotes: string;
  remainingTodayMotes: string;
  nextId: number;
}

export interface ProposeResult {
  decision: Decision;
  amountMotes: string;
  category: string;
  /** Hash of the on-chain transaction, parsed from livenet output. */
  txHash: string | null;
}

const HEX64 = /\b[0-9a-f]{64}\b/i;

function parseMarkedJson<T>(stdout: string): T {
  const line = stdout
    .split("\n")
    .map((l) => l.trim())
    .find((l) => l.startsWith("TG_JSON "));
  if (!line) {
    throw new Error(`no TG_JSON line in tg_cli output:\n${stdout}`);
  }
  return JSON.parse(line.slice("TG_JSON ".length)) as T;
}

/** Run a tg_cli subcommand and return its raw stdout. */
async function run(args: string[]): Promise<string> {
  const { stdout } = await execFileAsync(
    "cargo",
    ["run", "-q", "--features", "livenet", "--bin", "tg_cli", "--", ...args],
    { cwd: config.contractDir, maxBuffer: 16 * 1024 * 1024, env: process.env }
  );
  return stdout;
}

export async function getState(): Promise<VaultState> {
  return parseMarkedJson<VaultState>(await run(["state"]));
}

export async function propose(
  kind: "native" | "x402",
  recipientPubKeyHex: string,
  cspr: number,
  category: string,
  memo: string
): Promise<ProposeResult> {
  const stdout = await run([
    "propose",
    kind,
    recipientPubKeyHex,
    String(cspr),
    category,
    memo,
  ]);
  const parsed = parseMarkedJson<{ decision: Decision; amountMotes: string; category: string }>(
    stdout
  );
  // Prefer a tx hash that appears before our JSON marker (the settlement tx).
  const preamble = stdout.split("TG_JSON")[0] ?? stdout;
  const txHash = preamble.match(HEX64)?.[0] ?? null;
  return { ...parsed, txHash };
}

export async function approve(id: number): Promise<void> {
  await run(["approve", String(id)]);
}

export async function setAllowlist(pubKeyHex: string, allowed: boolean): Promise<void> {
  await run(["allowlist", pubKeyHex, String(allowed)]);
}

export async function deposit(cspr: number): Promise<void> {
  await run(["deposit", String(cspr)]);
}
