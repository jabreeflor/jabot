import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

export interface HostInfo {
  version: string;
  platform: string;
  host_mode: string;
}

function App() {
  const [host, setHost] = useState<HostInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<HostInfo>("host_health")
      .then(setHost)
      .catch((err) => setError(String(err)));
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
            <h2>Tauri 2 scaffold</h2>
            <p>
              Rust host in <code>src-tauri</code>, React 19 renderer via Vite.
              macOS overlay title bar and hide-to-Dock on window close (#4, #7).
            </p>
            {error && <p className="host-error">{error}</p>}
            {host && (
              <dl className="host-info">
                <dt>version</dt>
                <dd>{host.version}</dd>
                <dt>platform</dt>
                <dd>{host.platform}</dd>
                <dt>host_mode</dt>
                <dd>{host.host_mode}</dd>
              </dl>
            )}
          </div>
        </div>
      </main>
    </div>
  );
}

export default App;
