import { useEffect, useState } from "react";
import type { Health } from "./generated/Health";

// The UI is a pure projection of API state; types come from the ts-rs seam
// (ADR-1) — never hand-write shapes the server already defines.
export function App() {
  const [health, setHealth] = useState<Health | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetch("/healthz")
      .then((r) => r.json() as Promise<Health>)
      .then(setHealth)
      .catch((e) => setError(String(e)));
  }, []);

  return (
    <main>
      <h1>Surge</h1>
      {health && (
        <p>
          server v{health.version} · schema v{String(health.schema_version)}
        </p>
      )}
      {error && <p>server unreachable: {error}</p>}
    </main>
  );
}
