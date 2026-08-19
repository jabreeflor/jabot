import { useEffect, useState } from "react";
import { connectHost, type HelloResult, HostRpcError } from "./host";
import "./App.css";

function App() {
  const [hello, setHello] = useState<HelloResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    let disconnect: (() => void) | undefined;

    connectHost()
      .then(({ client, hello: result }) => {
        if (cancelled) {
          client.disconnect();
          return;
        }
        disconnect = () => client.disconnect();
        setHello(result);
      })
      .catch((err) => {
        if (!cancelled) setError(formatError(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
      disconnect?.();
    };
  }, []);

  return (
    <div className="app-shell">
      <div className="titlebar-drag" data-tauri-drag-region />
      <aside className="sidebar">
        <header className="sidebar-header">
          <span className="sidebar-title">JaBot</span>
        </header>
        <div className="sidebar-placeholder">
          Sidebar scaffold — crew, folders, and threads land in #11.
        </div>
      </aside>
      <main className="main">
        <header className="main-header">
          <h1 className="main-title">Welcome</h1>
        </header>
        <div className="main-content">
          <div className="scaffold-card">
            <h2>Host API</h2>
            <p>
              Typed JSON-RPC 2.0 between the React renderer and the in-process
              Rust host. Same messages will ride a Unix socket later (#8). The
              host owns SQLite (WAL) and an OS keychain vault for secrets (#9).
            </p>
            {error && (
              <p className="host-error" role="alert">
                {error}
              </p>
            )}
            {loading && !error && (
              <p className="host-loading" aria-live="polite">
                Connecting to host…
              </p>
            )}
            {hello && <HelloInfo hello={hello} />}
          </div>
        </div>
      </main>
    </div>
  );
}

function HelloInfo({ hello }: { hello: HelloResult }) {
  return (
    <dl className="host-info">
      <dt>protocol</dt>
      <dd>jsonrpc-2.0 / v{hello.protocolVersion}</dd>
      <dt>version</dt>
      <dd>{hello.version}</dd>
      <dt>platform</dt>
      <dd>{hello.platform}</dd>
      <dt>host_mode</dt>
      <dd>{hello.hostMode}</dd>
      <dt>host_id</dt>
      <dd>{hello.hostId}</dd>
      <dt>device_id</dt>
      <dd>{hello.device.deviceId}</dd>
      <dt>device_role</dt>
      <dd>{hello.device.role}</dd>
      {hello.storeError && (
        <>
          <dt>store_error</dt>
          <dd className="host-error">{hello.storeError}</dd>
        </>
      )}
      {hello.store && (
        <>
          <dt>sqlite</dt>
          <dd>
            v{hello.store.sqliteVersion} / {hello.store.journalMode} / schema{" "}
            {hello.store.schemaVersion}
          </dd>
          <dt>secrets</dt>
          <dd>{hello.store.secretsBackend}</dd>
          <dt>catalog</dt>
          <dd>
            {hello.store.harnessCount} harnesses · {hello.store.botCount} bots
          </dd>
        </>
      )}
    </dl>
  );
}

function formatError(err: unknown): string {
  if (err instanceof HostRpcError) {
    return `${err.message} (${err.code})`;
  }
  return String(err);
}

export default App;
