import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import dotenv from "dotenv";

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(__dirname, "..", "..");

// Load the shared project .env (testnet config) if present, then agent-local .env.
dotenv.config({ path: resolve(repoRoot, ".env") });
dotenv.config({ path: resolve(__dirname, "..", ".env"), override: true });

export interface Config {
  /** Absolute path to the Odra contract project (where tg_cli lives). */
  contractDir: string;
  /** Base URL of the x402-gated premium API. */
  mockApiUrl: string;
  /** The premium resource path the agent consumes. */
  resourcePath: string;
  /** Loop interval in ms (continuous mode). */
  intervalMs: number;
  /** When true, never touch the chain; simulate settlement locally. */
  dryRun: boolean;
  /** When true, run a single cycle and exit. */
  once: boolean;
  /**
   * Recipient (public key hex) the agent rebalances treasury into when market
   * risk is high. Should be on the vault allowlist for auto-execution.
   */
  reserveAccount: string;
}

const args = process.argv.slice(2);

export const config: Config = {
  contractDir: process.env.TG_CONTRACT_DIR ?? resolve(repoRoot, "contract"),
  mockApiUrl: process.env.MOCK_API_URL ?? "http://localhost:4021",
  resourcePath: process.env.X402_RESOURCE ?? "/api/market-intel",
  intervalMs: Number(process.env.AGENT_INTERVAL_MS ?? 20000),
  dryRun: args.includes("--dry-run") || process.env.AGENT_DRY_RUN === "1",
  once: args.includes("--once"),
  reserveAccount:
    process.env.TG_RESERVE_ACCOUNT ??
    "0202531fe6068134503d2723133227c867ac8fa6c83c537e9a44c3c5bdbdcb1fe7a0",
};

export function assertContractDir(): void {
  if (!existsSync(config.contractDir)) {
    throw new Error(`contract dir not found: ${config.contractDir}`);
  }
}
