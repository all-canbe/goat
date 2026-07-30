import { useState, useEffect, useCallback } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, Copy, X } from "lucide-react";
import { tauriInvoke } from "../../lib/tauri-bridge";

export default function WindowControls() {
  const [isMaximized, setIsMaximized] = useState(false);

  // 校验当前窗口是否最大化
  const checkMaximized = useCallback(async () => {
    try {
      const appWindow = getCurrentWindow();
      if (appWindow) {
        setIsMaximized(await appWindow.isMaximized());
      }
    } catch {
      // 在通用环境或桥接模式下静默忽略
    }
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    async function setupListener() {
      try {
        const appWindow = getCurrentWindow();
        if (appWindow) {
          setIsMaximized(await appWindow.isMaximized());
          unlisten = await appWindow.onResized(async () => {
            setIsMaximized(await appWindow.isMaximized());
          });
        }
      } catch {
        // 静默处理
      }
    }
    setupListener();
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  const handleMinimize = async (e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      await tauriInvoke("minimize_window");
    } catch {
      try {
        await getCurrentWindow().minimize();
      } catch (err) {
        console.warn("Minimize window failed:", err);
      }
    }
  };

  const handleToggleMaximize = async (e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      const res = await tauriInvoke<boolean>("toggle_maximize_window");
      setIsMaximized(Boolean(res));
    } catch {
      try {
        const appWindow = getCurrentWindow();
        await appWindow.toggleMaximize();
        await checkMaximized();
      } catch (err) {
        console.warn("Toggle maximize window failed:", err);
      }
    }
  };

  const handleClose = async (e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      await tauriInvoke("close_window");
    } catch {
      try {
        await getCurrentWindow().close();
      } catch (err) {
        console.warn("Close window failed:", err);
      }
    }
  };

  const noDragStyle = { WebkitAppRegion: "no-drag" } as React.CSSProperties;

  return (
    <div
      data-tauri-drag-region={false}
      style={noDragStyle}
      className="flex items-center h-full ml-1 shrink-0 select-none z-50"
    >
      <button
        onClick={handleMinimize}
        style={noDragStyle}
        className="h-full px-3 flex items-center justify-center text-text-secondary hover:text-text hover:bg-surface-hover transition-colors"
        title="最小化"
        aria-label="Minimize window"
      >
        <Minus size={13} />
      </button>
      <button
        onClick={handleToggleMaximize}
        style={noDragStyle}
        className="h-full px-3 flex items-center justify-center text-text-secondary hover:text-text hover:bg-surface-hover transition-colors"
        title={isMaximized ? "还原" : "最大化"}
        aria-label="Maximize or restore window"
      >
        {isMaximized ? <Copy size={12} /> : <Square size={12} />}
      </button>
      <button
        onClick={handleClose}
        style={noDragStyle}
        className="h-full px-3.5 flex items-center justify-center text-text-secondary hover:text-white hover:bg-error transition-colors"
        title="关闭"
        aria-label="Close window"
      >
        <X size={14} />
      </button>
    </div>
  );
}
