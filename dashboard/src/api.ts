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

export interface PendingAction {
  id: number;
  kind: "NativeTransfer" | "X402Payment";
  recipient: string;
  amountMotes: string;
  amountCspr: number;
  category: string;
  memo: string;
  createdAt: number;
  executed: boolean;
  vetoed: boolean;
}

async function jsonOrThrow(res: Response) {
  const body = await res.json();
  if (!res.ok) throw new Error((body as { error?: string }).error ?? res.statusText);
  return body;
}

export const api = {
  state: () => fetch("/api/state").then(jsonOrThrow) as Promise<VaultState>,
  pending: () =>
    fetch("/api/pending").then(jsonOrThrow) as Promise<{ items: PendingAction[]; nextId: number }>,
  approve: (id: number) => fetch(`/api/approve/${id}`, { method: "POST" }).then(jsonOrThrow),
  veto: (id: number) => fetch(`/api/veto/${id}`, { method: "POST" }).then(jsonOrThrow),
  setPolicy: (field: string, value: string) =>
    fetch("/api/policy", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ field, value }),
    }).then(jsonOrThrow),
  setAllowlist: (pubkey: string, allowed: boolean) =>
    fetch("/api/allowlist", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ pubkey, allowed }),
    }).then(jsonOrThrow),
};
