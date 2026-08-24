import { useEffect, useState, useCallback } from "react";
import Toolbar from "./components/Toolbar";
import RequestList from "./components/RequestList";
import RequestDetail from "./components/RequestDetail";
import AiPanel from "./components/AiPanel";
import Settings from "./components/Settings";
import { useAppStore } from "./lib/store";
import { RequestRecord } from "./lib/ipc";
import "./App.css";

function App() {
  const { refreshProxyStatus, refreshRequests, error, setError } = useAppStore();
  const [showSettings, setShowSettings] = useState(false);
  const [aiRecord, setAiRecord] = useState<RequestRecord | null>(null);
  const [showAi, setShowAi] = useState(false);

  // 监听 AI 分析事件
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail as RequestRecord;
      if (detail) {
        setAiRecord(detail);
        setShowAi(true);
      }
    };
    document.addEventListener("paxi:analyze", handler);
    return () => document.removeEventListener("paxi:analyze", handler);
  }, []);

  // 初始化：获取状态 + 定时轮询
  useEffect(() => {
    refreshProxyStatus();
    refreshRequests();
    const timer = setInterval(() => {
      refreshRequests();
    }, 2000);
    return () => clearInterval(timer);
  }, [refreshProxyStatus, refreshRequests]);

  const closeError = useCallback(() => setError(null), [setError]);

  return (
    <div className="app">
      <Toolbar onOpenSettings={() => setShowSettings(true)} />

      {error && (
        <div className="error-banner">
          <span>{error}</span>
          <button onClick={closeError}>✕</button>
        </div>
      )}

      <div className="main-area">
        <div className="left-pane">
          <RequestList />
        </div>
        <div className="right-pane">
          <RequestDetail />
        </div>
      </div>

      {showSettings && <Settings onClose={() => setShowSettings(false)} />}

      {showAi && (
        <div className="ai-overlay">
          <AiPanel record={aiRecord} onClose={() => setShowAi(false)} />
        </div>
      )}
    </div>
  );
}

export default App;
