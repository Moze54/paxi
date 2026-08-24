import { useEffect, useRef, useState } from "react";
import {
  Copy,
  Terminal,
  RotateCcw,
  Code2,
  Sparkles,
  FileDown,
} from "lucide-react";

export interface ContextMenuAction {
  key: string;
  label: string;
  icon?: React.ReactNode;
  onClick: () => void;
}

interface ContextMenuState {
  x: number;
  y: number;
  actions: ContextMenuAction[];
}

let hideFn: (() => void) | null = null;

/** 程序化关闭当前菜单（列表点击/滚动时调用） */
export function hideContextMenu() {
  hideFn?.();
}

export default function ContextMenu() {
  const [menu, setMenu] = useState<ContextMenuState | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const open = (e: Event) => {
      const detail = (e as CustomEvent).detail as ContextMenuState;
      setMenu(detail);
    };
    const close = () => setMenu(null);
    document.addEventListener("paxi:contextmenu", open);
    window.addEventListener("click", close);
    window.addEventListener("blur", close);
    window.addEventListener("resize", close);
    hideFn = close;
    return () => {
      document.removeEventListener("paxi:contextmenu", open);
      window.removeEventListener("click", close);
      window.removeEventListener("blur", close);
      window.removeEventListener("resize", close);
      hideFn = null;
    };
  }, []);

  if (!menu) return null;

  // 边界修正：避免菜单溢出屏幕
  const style = { left: menu.x, top: menu.y };

  return (
    <div
      ref={ref}
      className="context-menu"
      style={style}
      onClick={(e) => e.stopPropagation()}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
      }}
    >
      {menu.actions.map((a) => (
        <button key={a.key} className="context-menu-item" onClick={() => {
          a.onClick();
          setMenu(null);
        }}>
          {a.icon}
          <span>{a.label}</span>
        </button>
      ))}
    </div>
  );
}

/** 便捷：在指定坐标打开菜单 */
export function openContextMenu(x: number, y: number, actions: ContextMenuAction[]) {
  document.dispatchEvent(
    new CustomEvent("paxi:contextmenu", { detail: { x, y, actions } })
  );
}

/** 常用图标（供调用方使用） */
export const MENU_ICONS = {
  copy: <Copy size={13} />,
  curl: <Terminal size={13} />,
  replay: <RotateCcw size={13} />,
  code: <Code2 size={13} />,
  ai: <Sparkles size={13} />,
  har: <FileDown size={13} />,
};
