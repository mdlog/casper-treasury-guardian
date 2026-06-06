import { useCallback, useEffect, useState } from "react";
import { api, type PendingAction, type VaultState } from "./api.js";

function short(addr: string): string {
  if (!addr) return "—";
  const m = addr.replace(/^[A-Za-z-]*Key\(|\)$/g, "");
  return m.length > 22 ? `${m.slice(0, 12)}…${m.slice(-6)}` : m;
}

function Stat({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <div className="stat">
      <div className="stat-label">{label}</div>
      <div className="stat-value">{value}</div>
      {sub && <div className="stat-sub">{sub}</div>}
    </div>
  );
}

function Badge({ action }: { action: PendingAction }) {
  if (action.vetoed) return <span className="badge badge-veto">Vetoed</span>;
  if (action.executed) return <span className="badge badge-exec">Executed</span>;
  return <span className="badge badge-pending">Pending approval</span>;
}

export function App() {
  const [state, setState] = useState<VaultState | null>(null);
  const [pending, setPending] = useState<PendingAction[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [policy, setPolicy] = useState({ dailyCap: "", autoThreshold: "", cooldown: "" });
  const [allow, setAllow] = useState({ pubkey: "", allowed: true });

  const refresh = useCallback(async () => {
    try {
      const [s, p] = await Promise.all([api.state(), api.pending()]);
      setState(s);
      setPending(p.items.filter(Boolean));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
    const t = setInterval(refresh, 8000);
    return () => clearInterval(t);
  }, [refresh]);

  async function act<T>(fn: () => Promise<T>) {
    setBusy(true);
    try {
      await fn();
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  const queued = pending.filter((a) => !a.executed && !a.vetoed);
  const history = pending.filter((a) => a.executed || a.vetoed);

  return (
    <div className="wrap">
      <header>
        <h1>Treasury Guardian</h1>
        <p className="tagline">On-chain policy vault for autonomous treasury agents — owner console</p>
        {busy && <span className="busy">working…</span>}
      </header>

      {error && <div className="error">{error}</div>}

      {!state ? (
        <div className="card">
          Connecting to vault… ensure the dashboard API is running and
          <code> TG_CONTRACT_ADDRESS</code> is set.
        </div>
      ) : (
        <>
          <section className="grid">
            <Stat label="Vault balance" value={`${state.balanceCspr} CSPR`} />
            <Stat
              label="Daily cap"
              value={`${state.dailyCapCspr} CSPR`}
              sub={`remaining ${(Number(BigInt(state.remainingTodayMotes)) / 1e9).toFixed(2)} CSPR`}
            />
            <Stat label="Auto-exec threshold" value={`${state.autoThresholdCspr} CSPR`} />
            <Stat label="Cooldown" value={`${state.cooldownMs} ms`} />
            <Stat label="Require allowlist" value={state.requireAllowlist ? "yes" : "no"} />
            <Stat label="Paused" value={state.paused ? "yes" : "no"} />
            <Stat label="Owner" value={short(state.owner)} />
            <Stat label="Agent" value={short(state.agent)} />
          </section>

          <section className="card">
            <h2>Pending actions ({queued.length})</h2>
            {queued.length === 0 ? (
              <p className="muted">No actions awaiting approval.</p>
            ) : (
              <table>
                <thead>
                  <tr>
                    <th>#</th>
                    <th>Kind</th>
                    <th>Recipient</th>
                    <th>Amount</th>
                    <th>Category</th>
                    <th>Memo</th>
                    <th></th>
                  </tr>
                </thead>
                <tbody>
                  {queued.map((a) => (
                    <tr key={a.id}>
                      <td>{a.id}</td>
                      <td>{a.kind}</td>
                      <td title={a.recipient}>{short(a.recipient)}</td>
                      <td>{a.amountCspr} CSPR</td>
                      <td>{a.category}</td>
                      <td className="memo">{a.memo}</td>
                      <td className="actions">
                        <button disabled={busy} onClick={() => act(() => api.approve(a.id))}>
                          Approve
                        </button>
                        <button
                          className="danger"
                          disabled={busy}
                          onClick={() => act(() => api.veto(a.id))}
                        >
                          Veto
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </section>

          <div className="two-col">
            <section className="card">
              <h2>Policy</h2>
              {(["dailyCap", "autoThreshold", "cooldown"] as const).map((field) => (
                <div className="row" key={field}>
                  <label>
                    {field === "cooldown" ? "Cooldown (ms)" : `${field} (CSPR)`}
                  </label>
                  <input
                    value={policy[field]}
                    placeholder={
                      field === "dailyCap"
                        ? String(state.dailyCapCspr)
                        : field === "autoThreshold"
                        ? String(state.autoThresholdCspr)
                        : String(state.cooldownMs)
                    }
                    onChange={(e) => setPolicy({ ...policy, [field]: e.target.value })}
                  />
                  <button
                    disabled={busy || !policy[field]}
                    onClick={() =>
                      act(async () => {
                        await api.setPolicy(field, policy[field]);
                        setPolicy({ ...policy, [field]: "" });
                      })
                    }
                  >
                    Update
                  </button>
                </div>
              ))}
            </section>

            <section className="card">
              <h2>Allowlist</h2>
              <div className="row">
                <label>Public key</label>
                <input
                  value={allow.pubkey}
                  placeholder="0202…"
                  onChange={(e) => setAllow({ ...allow, pubkey: e.target.value })}
                />
              </div>
              <div className="row">
                <label>Allowed</label>
                <select
                  value={String(allow.allowed)}
                  onChange={(e) => setAllow({ ...allow, allowed: e.target.value === "true" })}
                >
                  <option value="true">true</option>
                  <option value="false">false</option>
                </select>
                <button
                  disabled={busy || !allow.pubkey}
                  onClick={() =>
                    act(async () => {
                      await api.setAllowlist(allow.pubkey, allow.allowed);
                      setAllow({ pubkey: "", allowed: true });
                    })
                  }
                >
                  Set
                </button>
              </div>
            </section>
          </div>

          <section className="card">
            <h2>Audit log ({history.length})</h2>
            {history.length === 0 ? (
              <p className="muted">No executed or vetoed actions yet.</p>
            ) : (
              <table>
                <thead>
                  <tr>
                    <th>#</th>
                    <th>Kind</th>
                    <th>Recipient</th>
                    <th>Amount</th>
                    <th>Category</th>
                    <th>Status</th>
                  </tr>
                </thead>
                <tbody>
                  {history.map((a) => (
                    <tr key={a.id}>
                      <td>{a.id}</td>
                      <td>{a.kind}</td>
                      <td title={a.recipient}>{short(a.recipient)}</td>
                      <td>{a.amountCspr} CSPR</td>
                      <td>{a.category}</td>
                      <td>
                        <Badge action={a} />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </section>
        </>
      )}
      <footer>Treasury Guardian · Casper Agentic Buildathon prototype</footer>
    </div>
  );
}
