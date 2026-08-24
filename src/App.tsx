import { useEffect, useState, useCallback } from "react";
import Toolbar from "./components/Toolbar";
import RequestList from "./components/RequestList";
import RequestDetail from "./components/RequestDetail";
import AiPanel from "./components/AiPanel";
import Settings from "./components/Settings";
import ConnectPhone from "./components/ConnectPhone";
import RulesPanel from "./components/RulesPanel";
import ReplayPanel from "./components/ReplayPanel";
import CodegenDialog from "./components/CodegenDialog";
import BreakpointPanel from "./components/BreakpointPanel";
import StatsPanel from "./components/StatsPanel";
import ContextMenu from "./components/ContextMenu";
import { useAppStore } from "./lib/store";
import { RequestRecord } from "./lib/ipc";
import "./App.css";

function App() {
  const { init, initTheme, error, setError, moveSelection, setFilter } = useAppStore();
  const [showSettings, setShowSettings] = useState(false);
  const [showConnect, setShowConnect] = useState(false);
  const [showRules, setShowRules] = useState(false);
  const [showStats, setShowStats] = useState(false);
  const [aiRecord, setAiRecord] = useState<RequestRecord | null>(null);
  const [showAi, setShowAi] = useState(false);
  const [replayRecord, setReplayRecord] = useState<RequestRecord | null>(null);
  const [codegenRecord, setCodegenRecord] = useState<RequestRecord | null>(null);

  // 监听 AI 分析 / 重放 / 代码生成事件
  useEffect(() => {
    const analyze = (e: Event) => {
      const detail = (e as CustomEvent).detail as RequestRecord;
      if (detail) {
        setAiRecord(detail);
        setShowAi(true);
      }
    };
    const replay = (e: Event) => {
      const detail = (e as CustomEvent).detail as RequestRecord;
      if (detail) setReplayRecord(detail);
    };
    const codegen = (e: Event) => {
      const detail = (e as CustomEvent).detail as RequestRecord;
      if (detail) setCodegenRecord(detail);
    };
    document.addEventListener("paxi:analyze", analyze);
    document.addEventListener("paxi:replay", replay);
    document.addEventListener("paxi:codegen", codegen);
    return () => {
      document.removeEventListener("paxi:analyze", analyze);
      document.removeEventListener("paxi:replay", replay);
      document.removeEventListener("paxi:codegen", codegen);
    };
  }, []);

  // 初始化：主题 + 事件订阅 + 初始数据（事件驱动，无轮询）
  useEffect(() => {
    initTheme();
    init();
  }, [initTheme, init]);

  // 全局快捷键
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      const inInput =
        target.tagName === "INPUT" ||
        target.tagName === "TEXTAREA" ||
        target.tagName === "SELECT" ||
        target.isContentEditable;

      // Esc：关闭最上层弹窗 / 取消输入焦点
      if (e.key === "Escape") {
        if (showAi) setShowAi(false);
        else if (codegenRecord) setCodegenRecord(null);
        else if (replayRecord) setReplayRecord(null);
        else if (showRules) setShowRules(false);
        else if (showStats) setShowStats(false);
        else if (showConnect) setShowConnect(false);
        else if (showSettings) setShowSettings(false);
        else if (inInput) (target as HTMLElement).blur();
        return;
      }

      // Ctrl+F：聚焦搜索框
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        const input = document.querySelector<HTMLInputElement>(".filter-input");
        input?.focus();
        input?.select();
        return;
      }

      // ↑↓：列表导航（输入框内不拦截，避免干扰编辑）
      if (!inInput && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
        e.preventDefault();
        moveSelection(e.key === "ArrowUp" ? -1 : 1);
        return;
      }

      // 输入框中 Ctrl+L 清空搜索
      if (inInput && (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "l") {
        e.preventDefault();
        setFilter("");
        return;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [showAi, showRules, showStats, showConnect, showSettings, codegenRecord, replayRecord, moveSelection, setFilter]);

  const closeError = useCallback(() => setError(null), [setError]);

  return (
    <div className="app">
      <Toolbar
        onOpenSettings={() => setShowSettings(true)}
        onOpenConnect={() => setShowConnect(true)}
        onOpenRules={() => setShowRules(true)}
        onOpenStats={() => setShowStats(true)}
      />

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
      {showConnect && <ConnectPhone onClose={() => setShowConnect(false)} />}
      {showRules && <RulesPanel onClose={() => setShowRules(false)} />}
      {showStats && <StatsPanel onClose={() => setShowStats(false)} />}
      {replayRecord && (
        <ReplayPanel record={replayRecord} onClose={() => setReplayRecord(null)} />
      )}
      {codegenRecord && (
        <CodegenDialog record={codegenRecord} onClose={() => setCodegenRecord(null)} />
      )}

      {/* 断点：命中规则挂起请求时自动弹出（内部空渲染不遮挡） */}
      <BreakpointPanel />

      {showAi && (
        <div className="ai-overlay">
          <AiPanel record={aiRecord} onClose={() => setShowAi(false)} />
        </div>
      )}

      <ContextMenu />
    </div>
  );
}

export default App;
