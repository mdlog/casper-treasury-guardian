/**
 * Dashboard backend. Wraps the Rust `tg_cli` livenet binary so the owner can
 * read vault state and approve/veto/configure from the browser. In production
 * the owner would sign these calls in-wallet via CSPR.click; for the prototype
 * the backend signs with the owner key configured in the project .env.
 */
import { execFile } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import cors from "cors";
import express, { type Request, type Response } from "express";

const execFileAsync = promisify(execFile);
const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(__dirname, "..", "..");

const PORT = Number(process.env.DASHBOARD_API_PORT ?? 4022);
const CONTRACT_DIR = process.env.TG_CONTRACT_DIR ?? resolve(repoRoot, "contract");

async function tg(args: string[]): Promise<{ json: unknown; raw: string }> {
  const { stdout } = await execFileAsync(
    "cargo",
    ["run", "-q", "--features", "livenet", "--bin", "tg_cli", "--", ...args],
    { cwd: CONTRACT_DIR, maxBuffer: 16 * 1024 * 1024, env: process.env }
  );
  const line = stdout
    .split("\n")
    .map((l) => l.trim())
    .find((l) => l.startsWith("TG_JSON "));
  return { json: line ? JSON.parse(line.slice("TG_JSON ".length)) : null, raw: stdout };
}

const app = express();
app.use(cors());
app.use(express.json());

function handle(fn: (req: Request) => Promise<unknown>) {
  return async (req: Request, res: Response) => {
    try {
      res.json(await fn(req));
    } catch (err) {
      console.error(err);
      res.status(500).json({ error: String(err) });
    }
  };
}

app.get("/api/health", (_req, res) => res.json({ ok: true }));

app.get(
  "/api/state",
  handle(async () => (await tg(["state"])).json)
);

// Collect every queued action up to nextId so the UI can show the audit log.
app.get(
  "/api/pending",
  handle(async () => {
    const state = (await tg(["state"])).json as { nextId: number } | null;
    const next = state?.nextId ?? 0;
    const items: unknown[] = [];
    for (let id = 0; id < next; id++) {
      items.push((await tg(["pending", String(id)])).json);
    }
    return { items, nextId: next };
  })
);

app.post(
  "/api/approve/:id",
  handle(async (req) => (await tg(["approve", req.params.id])).json)
);

app.post(
  "/api/veto/:id",
  handle(async (req) => (await tg(["veto", req.params.id])).json)
);

app.post(
  "/api/allowlist",
  handle(async (req) => {
    const { pubkey, allowed } = req.body as { pubkey: string; allowed: boolean };
    return (await tg(["allowlist", pubkey, String(Boolean(allowed))])).json;
  })
);

app.post(
  "/api/policy",
  handle(async (req) => {
    const { field, value } = req.body as { field: string; value: string };
    const map: Record<string, string> = {
      dailyCap: "set-daily-cap",
      autoThreshold: "set-auto-threshold",
      cooldown: "set-cooldown",
    };
    const cmd = map[field];
    if (!cmd) throw new Error(`unknown policy field: ${field}`);
    return (await tg([cmd, value])).json;
  })
);

app.listen(PORT, () => {
  console.log(`[dashboard-api] listening on http://localhost:${PORT}`);
  console.log(`[dashboard-api] contract dir: ${CONTRACT_DIR}`);
});
