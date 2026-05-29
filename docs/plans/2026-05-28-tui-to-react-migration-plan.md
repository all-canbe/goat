# TUI → React Web 前端迁移实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Python Textual TUI 替换为 React 18+ Web 前端，Python 后端通过 FastAPI + WebSocket 暴露 API，前端使用 Vite + TypeScript + Tailwind CSS + Zustand + shadcn/ui 构建。

**Architecture:** Monorepo 结构，Python 后端保留所有业务逻辑（EventBus、ConversationManager、LLM 调用、工具系统），新增 `my_tui/api/` 模块提供 FastAPI + WebSocket 服务。前端 `frontend/` 目录为独立 React 项目，构建产物由 FastAPI 直接 serve。`goat web` 一键启动。

**Tech Stack:** Python FastAPI + WebSocket, Vite 5, React 18, TypeScript 5, Tailwind CSS 3, shadcn/ui + Radix, Zustand

---

## 文件结构规划

```
my_tui/api/                          # 新增：FastAPI 后端
├── __init__.py                      # 空文件
├── server.py                        # FastAPI app 实例、CORS、静态文件挂载、启动
├── websocket.py                     # WSManager 连接管理、消息路由、EventBus 桥接
└── routes.py                        # REST API 路由

my_tui/tui/app.py                    # 修改：TuiScreen 保持不变，CLI 使用
my_tui/tui/bridge.py                 # 修改：桥接输出适配 WebSocket

frontend/                            # 新增：React 前端
├── package.json                     # 依赖声明
├── tsconfig.json                    # TypeScript 配置
├── vite.config.ts                   # Vite 配置（含代理）
├── tailwind.config.ts               # Tailwind 黑红主题
├── components.json                  # shadcn/ui 配置
├── index.html                       # 入口 HTML
├── src/
│   ├── main.tsx                     # React 入口
│   ├── App.tsx                      # 根组件，路由
│   ├── index.css                    # Tailwind 指令 + 自定义主题
│   ├── types/
│   │   └── index.ts                 # 共享类型定义
│   ├── lib/
│   │   └── ws-client.ts            # WebSocket 客户端封装
│   ├── stores/
│   │   ├── chatStore.ts            # 聊天状态
│   │   ├── sessionStore.ts         # 会话状态
│   │   └── connectionStore.ts      # 连接状态
│   ├── hooks/
│   │   ├── useWebSocket.ts         # WebSocket hook
│   │   └── useAutoScroll.ts        # 自动滚动 hook
│   ├── components/
│   │   ├── ui/                     # shadcn/ui 组件（自动生成）
│   │   ├── layout/
│   │   │   ├── AppLayout.tsx       # 主布局
│   │   │   ├── Sidebar.tsx         # 侧边栏
│   │   │   └── StatusBar.tsx       # 状态栏
│   │   ├── chat/
│   │   │   ├── ChatArea.tsx        # 聊天区域
│   │   │   ├── MessageList.tsx     # 消息列表
│   │   │   ├── UserMessage.tsx     # 用户消息
│   │   │   ├── AssistantMessage.tsx # 助手消息
│   │   │   ├── ToolCallCard.tsx    # 工具调用卡片
│   │   │   └── StreamingBubble.tsx # 流式气泡
│   │   ├── input/
│   │   │   └── InputPanel.tsx      # 输入面板
│   │   └── session/
│   │       ├── SessionList.tsx     # 会话列表
│   │       └── SessionItem.tsx     # 会话项
```

---

## 阶段一：后端 API 层

### Task 1: 创建 FastAPI 应用骨架

**Files:**
- Create: `my_tui/api/__init__.py`
- Create: `my_tui/api/server.py`

- [ ] **Step 1: 创建空 `__init__.py`**

```python
```

- [ ] **Step 2: 实现 `server.py` — FastAPI 应用初始化和启动**

```python
from __future__ import annotations

import sys
import webbrowser
from pathlib import Path

import uvicorn
from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles

from my_tui.api.routes import router
from my_tui.api.websocket import ws_manager


def create_app() -> FastAPI:
    app = FastAPI(title="Goat API", version="0.1.0")

    app.add_middleware(
        CORSMiddleware,
        allow_origins=["*"],
        allow_credentials=True,
        allow_methods=["*"],
        allow_headers=["*"],
    )

    app.include_router(router)

    frontend_dist = Path(__file__).resolve().parent.parent.parent / "frontend" / "dist"
    if frontend_dist.exists():
        app.mount("/", StaticFiles(directory=str(frontend_dist), html=True), name="static")

    return app


def start_server(host: str = "127.0.0.1", port: int = 8000, open_browser: bool = True) -> None:
    app = create_app()

    if open_browser:
        webbrowser.open(f"http://{host}:{port}")

    uvicorn.run(app, host=host, port=port, log_level="info")


if __name__ == "__main__":
    start_server()
```

- [ ] **Step 3: 添加 FastAPI 依赖到 `pyproject.toml`**

在 `pyproject.toml` 的 `[project]` 或 `[tool.poetry.dependencies]` 中找到 `dependencies` 列表，追加：

```toml
"fastapi>=0.115.0",
"uvicorn[standard]>=0.32.0",
```

- [ ] **Step 4: 验证导入**

```bash
python -c "from my_tui.api.server import create_app; print('OK')"
```

Expected: `OK`

- [ ] **Step 5: Commit**

```bash
git add my_tui/api/__init__.py my_tui/api/server.py pyproject.toml
git commit -m "feat: add FastAPI server skeleton with CORS and static file serving"
```

---

### Task 2: 实现 WebSocket 连接管理

**Files:**
- Create: `my_tui/api/websocket.py`

- [ ] **Step 1: 实现 `websocket.py`**

```python
from __future__ import annotations

import asyncio
import json
import logging
from typing import Any

from fastapi import WebSocket, WebSocketDisconnect

logger = logging.getLogger(__name__)


class WSManager:
    def __init__(self):
        self._connections: list[WebSocket] = []
        self._event_bus = None

    def set_event_bus(self, event_bus):
        self._event_bus = event_bus

    async def connect(self, websocket: WebSocket) -> None:
        await websocket.accept()
        self._connections.append(websocket)
        logger.info(f"WebSocket connected, total: {len(self._connections)}")

    def disconnect(self, websocket: WebSocket) -> None:
        if websocket in self._connections:
            self._connections.remove(websocket)
        logger.info(f"WebSocket disconnected, total: {len(self._connections)}")

    async def broadcast(self, message: dict[str, Any]) -> None:
        dead: list[WebSocket] = []
        for ws in self._connections:
            try:
                await ws.send_json(message)
            except Exception:
                dead.append(ws)
        for ws in dead:
            self._connections.remove(ws)

    async def handle_message(self, websocket: WebSocket, data: dict[str, Any]) -> None:
        msg_type = data.get("type", "")
        payload = data.get("payload", {})

        if msg_type == "chat.send":
            await self._handle_chat_send(payload)
        elif msg_type == "chat.cancel":
            await self._handle_chat_cancel()
        elif msg_type == "tool.approve":
            await self._handle_tool_approve(payload)
        elif msg_type == "tool.reject":
            await self._handle_tool_reject(payload)
        else:
            logger.warning(f"Unknown message type: {msg_type}")

    async def _handle_chat_send(self, payload: dict) -> None:
        text = payload.get("text", "")
        if self._event_bus:
            self._event_bus.publish_nowait(
                "user", "user_input", text, agent_name="user"
            )

    async def _handle_chat_cancel(self) -> None:
        if self._event_bus:
            self._event_bus.publish_nowait(
                "system", "cancel", "cancelled", agent_name="system"
            )

    async def _handle_tool_approve(self, payload: dict) -> None:
        tool_call_id = payload.get("toolCallId", "")
        if self._event_bus:
            self._event_bus.publish_nowait(
                "user", "tool_approve", tool_call_id, agent_name="user"
            )

    async def _handle_tool_reject(self, payload: dict) -> None:
        tool_call_id = payload.get("toolCallId", "")
        if self._event_bus:
            self._event_bus.publish_nowait(
                "user", "tool_reject", tool_call_id, agent_name="user"
            )


ws_manager = WSManager()
```

- [ ] **Step 2: 验证导入**

```bash
python -c "from my_tui.api.websocket import ws_manager; print('OK')"
```

Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add my_tui/api/websocket.py
git commit -m "feat: add WebSocket manager with EventBus bridge"
```

---

### Task 3: 实现 REST API 路由

**Files:**
- Create: `my_tui/api/routes.py`

- [ ] **Step 1: 实现 `routes.py`**

```python
from __future__ import annotations

import json
import asyncio
from typing import Any

from fastapi import APIRouter, WebSocket, WebSocketDisconnect

from my_tui.api.websocket import ws_manager

router = APIRouter(prefix="/api")


@router.get("/health")
async def health_check():
    return {"status": "ok"}


@router.get("/sessions")
async def list_sessions():
    return {"sessions": []}


@router.post("/sessions")
async def create_session():
    return {"sessionId": "new-session", "title": "New Session"}


@router.delete("/sessions/{session_id}")
async def delete_session(session_id: str):
    return {"deleted": session_id}


@router.get("/sessions/{session_id}/messages")
async def get_messages(session_id: str):
    return {"sessionId": session_id, "messages": []}


@router.websocket("/ws")
async def websocket_endpoint(websocket: WebSocket):
    await ws_manager.connect(websocket)
    try:
        while True:
            raw = await websocket.receive_text()
            try:
                data = json.loads(raw)
            except json.JSONDecodeError:
                continue
            await ws_manager.handle_message(websocket, data)
    except WebSocketDisconnect:
        ws_manager.disconnect(websocket)
```

- [ ] **Step 2: 验证启动**

```bash
python -c "from my_tui.api.server import create_app; app = create_app(); print('OK')"
```

Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add my_tui/api/routes.py
git commit -m "feat: add REST API routes and WebSocket endpoint"
```

---

### Task 4: EventBus 到 WebSocket 事件桥接

**Files:**
- Modify: `my_tui/api/websocket.py` — 新增 `setup_event_bridge()` 函数
- Modify: `my_tui/api/server.py` — 在 `create_app()` 中调用桥接初始化

- [ ] **Step 1: 在 `websocket.py` 末尾追加事件桥接函数**

```python
def setup_event_bridge(event_bus):
    """将 EventBus 事件桥接到 WebSocket 广播。"""
    ws_manager.set_event_bus(event_bus)

    def on_llm_stream(agent_name, event_type, data):
        asyncio.create_task(ws_manager.broadcast({
            "type": "chat.stream",
            "payload": {"token": data, "messageId": ""},
        }))

    def on_llm_response(agent_name, event_type, data):
        asyncio.create_task(ws_manager.broadcast({
            "type": "chat.response",
            "payload": {"messageId": "", "content": data},
        }))

    def on_error(agent_name, event_type, data):
        asyncio.create_task(ws_manager.broadcast({
            "type": "chat.error",
            "payload": {"messageId": "", "error": str(data)},
        }))

    def on_tool_start(agent_name, event_type, data):
        asyncio.create_task(ws_manager.broadcast({
            "type": "tool.start",
            "payload": {"toolName": data.get("name", ""), "args": data.get("args", {})},
        }))

    def on_tool_complete(agent_name, event_type, data):
        asyncio.create_task(ws_manager.broadcast({
            "type": "tool.complete",
            "payload": {"toolName": data.get("name", ""), "result": data.get("result", "")},
        }))

    def on_status_update(agent_name, event_type, data):
        asyncio.create_task(ws_manager.broadcast({
            "type": "status.update",
            "payload": {"model": "", "provider": "", "mode": str(data)},
        }))

    from my_tui.core.event_bus import EventType
    event_bus.subscribe("llm", EventType.LLM_STREAM, on_llm_stream)
    event_bus.subscribe("llm", EventType.LLM_RESPONSE, on_llm_response)
    event_bus.subscribe("system", EventType.ERROR, on_error)
    event_bus.subscribe("system", EventType.TOOL_START, on_tool_start)
    event_bus.subscribe("system", EventType.TOOL_COMPLETE, on_tool_complete)
    event_bus.subscribe("system", EventType.COMPLETED, on_status_update)
```

- [ ] **Step 2: 在 `server.py` 的 `create_app()` 中添加 lifespan 事件**

在 `create_app()` 函数末尾（`return app` 之前），添加：

```python
from contextlib import asynccontextmanager

@asynccontextmanager
async def lifespan(app: FastAPI):
    yield

app.router.lifespan_context = lifespan
```

- [ ] **Step 3: 验证导入**

```bash
python -c "from my_tui.api.websocket import setup_event_bridge; print('OK')"
```

Expected: `OK`

- [ ] **Step 4: Commit**

```bash
git add my_tui/api/websocket.py my_tui/api/server.py
git commit -m "feat: add EventBus to WebSocket event bridge"
```

---

### Task 5: 实现 `goat web` 启动命令

**Files:**
- Modify: `main.py` — 添加 `web` 子命令

- [ ] **Step 1: 查看现有 CLI 入口**

先读取 `main.py` 了解现有命令结构，然后添加 web 子命令。

```bash
python -c "import main; print(dir(main))"
```

- [ ] **Step 2: 添加 web 子命令逻辑**

在 `main.py` 中添加：

```python
def web_command(args=None):
    """启动 Web 前端模式"""
    from my_tui.api.server import start_server
    start_server(open_browser=True)
```

在现有的 CLI 参数解析中，添加 `web` 子命令：

```python
if sys.argv[1] == "web":
    web_command()
elif sys.argv[1] == "cli":
    cli_command()
```

- [ ] **Step 3: 验证命令**

```bash
python main.py web --help  # 应显示帮助或启动
```

- [ ] **Step 4: Commit**

```bash
git add main.py
git commit -m "feat: add `goat web` command for web frontend mode"
```

---

## 阶段二：前端项目搭建

### Task 6: 初始化 Vite + React + TypeScript 项目

**Files:**
- Create: `frontend/package.json`
- Create: `frontend/tsconfig.json`
- Create: `frontend/tsconfig.node.json`
- Create: `frontend/vite.config.ts`
- Create: `frontend/index.html`
- Create: `frontend/src/main.tsx`
- Create: `frontend/src/App.tsx`
- Create: `frontend/src/index.css`

- [ ] **Step 1: 创建 `package.json`**

```json
{
  "name": "goat-frontend",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc -b && vite build",
    "preview": "vite preview"
  },
  "dependencies": {
    "react": "^18.3.1",
    "react-dom": "^18.3.1",
    "react-markdown": "^9.0.1",
    "rehype-highlight": "^7.0.0",
    "zustand": "^5.0.0",
    "lucide-react": "^0.460.0",
    "class-variance-authority": "^0.7.1",
    "clsx": "^2.1.1",
    "tailwind-merge": "^2.6.0"
  },
  "devDependencies": {
    "@types/react": "^18.3.12",
    "@types/react-dom": "^18.3.1",
    "@vitejs/plugin-react": "^4.3.4",
    "autoprefixer": "^10.4.20",
    "postcss": "^8.4.49",
    "tailwindcss": "^3.4.17",
    "typescript": "^5.6.3",
    "vite": "^5.4.11"
  }
}
```

- [ ] **Step 2: 创建 `tsconfig.json`**

```json
{
  "compilerOptions": {
    "target": "ES2020",
    "useDefineForClassFields": true,
    "lib": ["ES2020", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "isolatedModules": true,
    "moduleDetection": "force",
    "noEmit": true,
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true,
    "forceConsistentCasingInFileNames": true,
    "baseUrl": ".",
    "paths": {
      "@/*": ["./src/*"]
    }
  },
  "include": ["src"]
}
```

- [ ] **Step 3: 创建 `vite.config.ts`**

```typescript
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    port: 5173,
    proxy: {
      '/api': 'http://127.0.0.1:8000',
      '/ws': {
        target: 'ws://127.0.0.1:8000',
        ws: true,
      },
    },
  },
})
```

- [ ] **Step 4: 创建 `index.html`**

```html
<!DOCTYPE html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8" />
    <link rel="icon" type="image/svg+xml" href="/vite.svg" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Goat</title>
  </head>
  <body class="bg-bg text-text">
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

- [ ] **Step 5: 创建 `src/main.tsx`**

```tsx
import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import './index.css'

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)
```

- [ ] **Step 6: 创建 `src/App.tsx`**

```tsx
function App() {
  return (
    <div className="h-screen flex items-center justify-center">
      <p className="text-text-dim">Goat Web Frontend</p>
    </div>
  )
}

export default App
```

- [ ] **Step 7: 创建 `src/index.css`**

```css
@tailwind base;
@tailwind components;
@tailwind utilities;

@layer base {
  body {
    @apply bg-bg text-text;
    font-family: 'JetBrains Mono', 'Fira Code', 'Consolas', monospace;
  }
}
```

- [ ] **Step 8: 安装依赖并验证**

```bash
cd frontend && npm install
```

Expected: 安装成功

```bash
cd frontend && npx tsc --noEmit
```

Expected: 无类型错误

- [ ] **Step 9: Commit**

```bash
git add frontend/
git commit -m "feat: initialize Vite + React + TypeScript frontend project"
```

---

### Task 7: 配置 Tailwind CSS 黑红主题

**Files:**
- Create: `frontend/tailwind.config.ts`
- Create: `frontend/postcss.config.js`

- [ ] **Step 1: 创建 `postcss.config.js`**

```javascript
export default {
  plugins: {
    tailwindcss: {},
    autoprefixer: {},
  },
}
```

- [ ] **Step 2: 创建 `tailwind.config.ts`**

```typescript
import type { Config } from 'tailwindcss'

const config: Config = {
  content: ['./index.html', './src/**/*.{js,ts,jsx,tsx}'],
  theme: {
    extend: {
      colors: {
        bg: '#0a0a0a',
        surface: '#1a1a1a',
        'surface-light': '#2a2a2a',
        'surface-lighter': '#3a3a3a',
        primary: '#cc0000',
        'primary-light': '#ff1a1a',
        'primary-dim': '#880000',
        'primary-glow': '#ff3333',
        text: '#e0e0e0',
        'text-dim': '#888888',
        'text-darker': '#555555',
        'text-bright': '#ffffff',
        success: '#00cc66',
        warning: '#ff6600',
        error: '#ff0000',
        border: '#3a0a0a',
        'border-light': '#5a1a1a',
        'border-focus': '#cc0000',
        scrollbar: '#4a0a0a',
        'scrollbar-hover': '#6a1a1a',
        'selection-bg': '#330000',
        'selection-fg': '#ff6666',
        'mode-plan': '#ff6600',
        'mode-agent': '#cc0000',
        'mode-yolo': '#ff0000',
      },
      fontFamily: {
        mono: ['JetBrains Mono', 'Fira Code', 'Consolas', 'monospace'],
      },
    },
  },
  plugins: [],
}

export default config
```

- [ ] **Step 3: 验证编译**

```bash
cd frontend && npx tailwindcss -i src/index.css -o /dev/null --dry-run
```

Expected: 无错误

- [ ] **Step 4: Commit**

```bash
git add frontend/tailwind.config.ts frontend/postcss.config.js
git commit -m "feat: configure Tailwind CSS with RedBlack theme"
```

---

### Task 8: 共享类型定义和 WebSocket 客户端

**Files:**
- Create: `frontend/src/types/index.ts`
- Create: `frontend/src/lib/ws-client.ts`

- [ ] **Step 1: 创建 `src/types/index.ts`**

```typescript
export interface WSMessage {
  type: string
  payload: Record<string, unknown>
  timestamp?: number
}

export interface Message {
  id: string
  role: 'user' | 'assistant' | 'system'
  content: string
  toolCalls?: ToolCall[]
  timestamp: number
}

export interface ToolCall {
  name: string
  args: Record<string, unknown>
  result?: string
  status: 'running' | 'complete' | 'error'
}

export interface Session {
  id: string
  title: string
  createdAt: string
  messageCount: number
}

export type ConnectionStatus = 'connecting' | 'connected' | 'disconnected'
export type PermissionMode = 'plan' | 'agent' | 'yolo'
```

- [ ] **Step 2: 创建 `src/lib/ws-client.ts`**

```typescript
type MessageHandler = (msg: WSMessage) => void
type StatusHandler = (status: ConnectionStatus) => void

export class WSClient {
  private ws: WebSocket | null = null
  private url: string
  private handlers: Map<string, Set<MessageHandler>> = new Map()
  private statusHandlers: Set<StatusHandler> = new Set()
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private _status: ConnectionStatus = 'disconnected'

  constructor(url: string = '') {
    this.url = url || this.detectUrl()
  }

  get status(): ConnectionStatus {
    return this._status
  }

  private detectUrl(): string {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    return `${protocol}//${window.location.host}/ws`
  }

  connect(): void {
    if (this.ws?.readyState === WebSocket.OPEN) return
    this.setStatus('connecting')

    this.ws = new WebSocket(this.url)
    this.ws.onopen = () => {
      this.setStatus('connected')
    }
    this.ws.onclose = () => {
      this.setStatus('disconnected')
      this.scheduleReconnect()
    }
    this.ws.onerror = () => {
      this.setStatus('disconnected')
    }
    this.ws.onmessage = (event) => {
      try {
        const msg: WSMessage = JSON.parse(event.data)
        const handlers = this.handlers.get(msg.type)
        if (handlers) {
          handlers.forEach((h) => h(msg))
        }
      } catch {
        // ignore malformed messages
      }
    }
  }

  private scheduleReconnect(): void {
    if (this.reconnectTimer) return
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null
      this.connect()
    }, 3000)
  }

  disconnect(): void {
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }
    this.ws?.close()
    this.ws = null
    this.setStatus('disconnected')
  }

  send(type: string, payload: Record<string, unknown> = {}): void {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify({ type, payload }))
    }
  }

  on(type: string, handler: MessageHandler): () => void {
    if (!this.handlers.has(type)) {
      this.handlers.set(type, new Set())
    }
    this.handlers.get(type)!.add(handler)
    return () => {
      this.handlers.get(type)?.delete(handler)
    }
  }

  onStatusChange(handler: StatusHandler): () => void {
    this.statusHandlers.add(handler)
    return () => {
      this.statusHandlers.delete(handler)
    }
  }

  private setStatus(status: ConnectionStatus): void {
    this._status = status
    this.statusHandlers.forEach((h) => h(status))
  }
}
```

- [ ] **Step 3: 验证编译**

```bash
cd frontend && npx tsc --noEmit
```

Expected: 无类型错误

- [ ] **Step 4: Commit**

```bash
git add frontend/src/types/ frontend/src/lib/
git commit -m "feat: add shared types and WebSocket client"
```

---

### Task 9: Zustand Stores

**Files:**
- Create: `frontend/src/stores/chatStore.ts`
- Create: `frontend/src/stores/sessionStore.ts`
- Create: `frontend/src/stores/connectionStore.ts`

- [ ] **Step 1: 创建 `src/stores/chatStore.ts`**

```typescript
import { create } from 'zustand'
import type { Message } from '@/types'

interface ChatStore {
  messages: Message[]
  streamingContent: string
  isStreaming: boolean
  addMessage: (msg: Message) => void
  appendStreamToken: (token: string) => void
  commitStream: (messageId: string) => void
  clearStream: () => void
  clearMessages: () => void
}

export const useChatStore = create<ChatStore>((set, get) => ({
  messages: [],
  streamingContent: '',
  isStreaming: false,

  addMessage: (msg) =>
    set((state) => ({ messages: [...state.messages, msg] })),

  appendStreamToken: (token) =>
    set((state) => ({
      streamingContent: state.streamingContent + token,
      isStreaming: true,
    })),

  commitStream: (messageId) => {
    const content = get().streamingContent
    if (!content) return
    const msg: Message = {
      id: messageId || crypto.randomUUID(),
      role: 'assistant',
      content,
      timestamp: Date.now(),
    }
    set((state) => ({
      messages: [...state.messages, msg],
      streamingContent: '',
      isStreaming: false,
    }))
  },

  clearStream: () =>
    set({ streamingContent: '', isStreaming: false }),

  clearMessages: () =>
    set({ messages: [], streamingContent: '', isStreaming: false }),
}))
```

- [ ] **Step 2: 创建 `src/stores/sessionStore.ts`**

```typescript
import { create } from 'zustand'
import type { Session } from '@/types'

interface SessionStore {
  sessions: Session[]
  activeSessionId: string | null
  isLoading: boolean
  setActiveSession: (id: string) => void
  createSession: () => Promise<void>
  deleteSession: (id: string) => Promise<void>
  loadSessions: () => Promise<void>
}

export const useSessionStore = create<SessionStore>((set) => ({
  sessions: [],
  activeSessionId: null,
  isLoading: false,

  setActiveSession: (id) => set({ activeSessionId: id }),

  createSession: async () => {
    const res = await fetch('/api/sessions', { method: 'POST' })
    const data = await res.json()
    set((state) => ({
      sessions: [
        { id: data.sessionId, title: data.title, createdAt: new Date().toISOString(), messageCount: 0 },
        ...state.sessions,
      ],
      activeSessionId: data.sessionId,
    }))
  },

  deleteSession: async (id) => {
    await fetch(`/api/sessions/${id}`, { method: 'DELETE' })
    set((state) => ({
      sessions: state.sessions.filter((s) => s.id !== id),
      activeSessionId: state.activeSessionId === id ? null : state.activeSessionId,
    }))
  },

  loadSessions: async () => {
    set({ isLoading: true })
    try {
      const res = await fetch('/api/sessions')
      const data = await res.json()
      set({ sessions: data.sessions || [], isLoading: false })
    } catch {
      set({ isLoading: false })
    }
  },
}))
```

- [ ] **Step 3: 创建 `src/stores/connectionStore.ts`**

```typescript
import { create } from 'zustand'
import type { ConnectionStatus } from '@/types'

interface ConnectionStore {
  status: ConnectionStatus
  setStatus: (status: ConnectionStatus) => void
}

export const useConnectionStore = create<ConnectionStore>((set) => ({
  status: 'disconnected',
  setStatus: (status) => set({ status }),
}))
```

- [ ] **Step 4: 验证编译**

```bash
cd frontend && npx tsc --noEmit
```

Expected: 无类型错误

- [ ] **Step 5: Commit**

```bash
git add frontend/src/stores/
git commit -m "feat: add Zustand stores (chat, session, connection)"
```

---

### Task 10: 自定义 Hooks

**Files:**
- Create: `frontend/src/hooks/useWebSocket.ts`
- Create: `frontend/src/hooks/useAutoScroll.ts`

- [ ] **Step 1: 创建 `src/hooks/useWebSocket.ts`**

```typescript
import { useEffect, useRef } from 'react'
import { WSClient } from '@/lib/ws-client'
import { useChatStore } from '@/stores/chatStore'
import { useConnectionStore } from '@/stores/connectionStore'

export function useWebSocket() {
  const clientRef = useRef<WSClient | null>(null)
  const appendStreamToken = useChatStore((s) => s.appendStreamToken)
  const commitStream = useChatStore((s) => s.commitStream)
  const addMessage = useChatStore((s) => s.addMessage)
  const setStatus = useConnectionStore((s) => s.setStatus)

  useEffect(() => {
    const client = new WSClient()
    clientRef.current = client

    const unsubStatus = client.onStatusChange((status) => {
      setStatus(status)
    })

    const unsubStream = client.on('chat.stream', (msg) => {
      const token = msg.payload.token as string
      if (token) appendStreamToken(token)
    })

    const unsubResponse = client.on('chat.response', (msg) => {
      const messageId = msg.payload.messageId as string
      commitStream(messageId)
    })

    const unsubError = client.on('chat.error', (msg) => {
      const error = msg.payload.error as string
      addMessage({
        id: crypto.randomUUID(),
        role: 'system',
        content: `Error: ${error}`,
        timestamp: Date.now(),
      })
      useChatStore.getState().clearStream()
    })

    client.connect()

    return () => {
      unsubStatus()
      unsubStream()
      unsubResponse()
      unsubError()
      client.disconnect()
    }
  }, [])

  return clientRef.current
}
```

- [ ] **Step 2: 创建 `src/hooks/useAutoScroll.ts`**

```typescript
import { useEffect, useRef } from 'react'
import { useChatStore } from '@/stores/chatStore'

export function useAutoScroll() {
  const containerRef = useRef<HTMLDivElement>(null)
  const messages = useChatStore((s) => s.messages)
  const streamingContent = useChatStore((s) => s.streamingContent)

  useEffect(() => {
    const el = containerRef.current
    if (el) {
      el.scrollTo({ top: el.scrollHeight, behavior: 'smooth' })
    }
  }, [messages, streamingContent])

  return containerRef
}
```

- [ ] **Step 3: 验证编译**

```bash
cd frontend && npx tsc --noEmit
```

Expected: 无类型错误

- [ ] **Step 4: Commit**

```bash
git add frontend/src/hooks/
git commit -m "feat: add useWebSocket and useAutoScroll hooks"
```

---

## 阶段三：核心组件开发

### Task 11: 布局组件 (AppLayout, Sidebar, StatusBar)

**Files:**
- Create: `frontend/src/components/layout/AppLayout.tsx`
- Create: `frontend/src/components/layout/Sidebar.tsx`
- Create: `frontend/src/components/layout/StatusBar.tsx`

- [ ] **Step 1: 创建 `StatusBar.tsx`**

```tsx
import { useConnectionStore } from '@/stores/connectionStore'

export default function StatusBar() {
  const status = useConnectionStore((s) => s.status)

  const statusColors: Record<string, string> = {
    connected: 'text-success',
    connecting: 'text-warning',
    disconnected: 'text-error',
  }

  return (
    <div className="h-7 bg-surface border-t border-border flex items-center px-3 text-xs text-text-dim gap-4">
      <span className="text-text-darker">GOAT</span>
      <span className={statusColors[status] || 'text-text-dim'}>
        ● {status}
      </span>
      <span className="ml-auto text-text-darker">v0.1.0</span>
    </div>
  )
}
```

- [ ] **Step 2: 创建 `Sidebar.tsx`**

```tsx
import { Plus } from 'lucide-react'
import { useSessionStore } from '@/stores/sessionStore'
import SessionList from '@/components/session/SessionList'

export default function Sidebar() {
  const createSession = useSessionStore((s) => s.createSession)

  return (
    <div className="w-[260px] bg-surface border-r border-border flex flex-col h-full">
      <div className="p-3 border-b border-border flex items-center justify-between">
        <span className="text-primary-light font-bold text-sm">GOAT</span>
        <button
          onClick={createSession}
          className="p-1 rounded hover:bg-surface-light text-text-dim hover:text-text transition-colors"
          title="新建会话"
        >
          <Plus size={16} />
        </button>
      </div>
      <div className="flex-1 overflow-y-auto">
        <SessionList />
      </div>
    </div>
  )
}
```

- [ ] **Step 3: 创建 `AppLayout.tsx`**

```tsx
import { ReactNode } from 'react'
import Sidebar from './Sidebar'
import StatusBar from './StatusBar'

interface AppLayoutProps {
  children: ReactNode
}

export default function AppLayout({ children }: AppLayoutProps) {
  return (
    <div className="h-screen flex flex-col bg-bg">
      <div className="flex-1 flex overflow-hidden">
        <Sidebar />
        <main className="flex-1 flex flex-col overflow-hidden">
          {children}
        </main>
      </div>
      <StatusBar />
    </div>
  )
}
```

- [ ] **Step 4: 验证编译**

```bash
cd frontend && npx tsc --noEmit
```

Expected: 无类型错误

- [ ] **Step 5: Commit**

```bash
git add frontend/src/components/layout/ frontend/src/components/session/
git commit -m "feat: add layout components (AppLayout, Sidebar, StatusBar)"
```

---

### Task 12: 消息组件 (MessageList, UserMessage, AssistantMessage, StreamingBubble)

**Files:**
- Create: `frontend/src/components/chat/MessageList.tsx`
- Create: `frontend/src/components/chat/UserMessage.tsx`
- Create: `frontend/src/components/chat/AssistantMessage.tsx`
- Create: `frontend/src/components/chat/StreamingBubble.tsx`

- [ ] **Step 1: 创建 `UserMessage.tsx`**

```tsx
import { User } from 'lucide-react'

interface UserMessageProps {
  content: string
}

export default function UserMessage({ content }: UserMessageProps) {
  return (
    <div className="flex gap-3 px-4 py-3">
      <div className="w-6 h-6 rounded bg-primary-dim flex items-center justify-center flex-shrink-0 mt-0.5">
        <User size={14} className="text-primary-light" />
      </div>
      <div className="flex-1 min-w-0">
        <p className="text-text whitespace-pre-wrap break-words">{content}</p>
      </div>
    </div>
  )
}
```

- [ ] **Step 2: 创建 `AssistantMessage.tsx`**

```tsx
import ReactMarkdown from 'react-markdown'
import { Bot } from 'lucide-react'

interface AssistantMessageProps {
  content: string
}

export default function AssistantMessage({ content }: AssistantMessageProps) {
  return (
    <div className="flex gap-3 px-4 py-3 bg-surface/50">
      <div className="w-6 h-6 rounded bg-primary-dim/50 flex items-center justify-center flex-shrink-0 mt-0.5">
        <Bot size={14} className="text-primary" />
      </div>
      <div className="flex-1 min-w-0 prose prose-invert prose-sm max-w-none">
        <ReactMarkdown>{content}</ReactMarkdown>
      </div>
    </div>
  )
}
```

- [ ] **Step 3: 创建 `StreamingBubble.tsx`**

```tsx
import ReactMarkdown from 'react-markdown'
import { Bot } from 'lucide-react'

interface StreamingBubbleProps {
  content: string
}

export default function StreamingBubble({ content }: StreamingBubbleProps) {
  if (!content) return null

  return (
    <div className="flex gap-3 px-4 py-3 bg-surface/50">
      <div className="w-6 h-6 rounded bg-primary-dim/50 flex items-center justify-center flex-shrink-0 mt-0.5">
        <Bot size={14} className="text-primary animate-pulse" />
      </div>
      <div className="flex-1 min-w-0 prose prose-invert prose-sm max-w-none">
        <ReactMarkdown>{content}</ReactMarkdown>
        <span className="inline-block w-2 h-4 bg-primary-light animate-pulse ml-0.5 align-middle" />
      </div>
    </div>
  )
}
```

- [ ] **Step 4: 创建 `MessageList.tsx`**

```tsx
import { useChatStore } from '@/stores/chatStore'
import { useAutoScroll } from '@/hooks/useAutoScroll'
import UserMessage from './UserMessage'
import AssistantMessage from './AssistantMessage'
import StreamingBubble from './StreamingBubble'

export default function MessageList() {
  const messages = useChatStore((s) => s.messages)
  const streamingContent = useChatStore((s) => s.streamingContent)
  const isStreaming = useChatStore((s) => s.isStreaming)
  const containerRef = useAutoScroll()

  return (
    <div
      ref={containerRef}
      className="flex-1 overflow-y-auto scrollbar-thin"
    >
      {messages.length === 0 && !isStreaming && (
        <div className="flex items-center justify-center h-full text-text-darker text-sm">
          <p>输入消息开始对话...</p>
        </div>
      )}
      {messages.map((msg) =>
        msg.role === 'user' ? (
          <UserMessage key={msg.id} content={msg.content} />
        ) : (
          <AssistantMessage key={msg.id} content={msg.content} />
        )
      )}
      {isStreaming && streamingContent && (
        <StreamingBubble content={streamingContent} />
      )}
    </div>
  )
}
```

- [ ] **Step 5: 验证编译**

```bash
cd frontend && npx tsc --noEmit
```

Expected: 无类型错误

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/chat/
git commit -m "feat: add message components (MessageList, UserMessage, AssistantMessage, StreamingBubble)"
```

---

### Task 13: 输入面板组件 (InputPanel)

**Files:**
- Create: `frontend/src/components/input/InputPanel.tsx`

- [ ] **Step 1: 创建 `InputPanel.tsx`**

```tsx
import { useState, useRef, KeyboardEvent } from 'react'
import { Send, Square } from 'lucide-react'
import { useChatStore } from '@/stores/chatStore'
import { useSessionStore } from '@/stores/sessionStore'

export default function InputPanel() {
  const [text, setText] = useState('')
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const isStreaming = useChatStore((s) => s.isStreaming)
  const activeSessionId = useSessionStore((s) => s.activeSessionId)

  const handleSubmit = () => {
    const trimmed = text.trim()
    if (!trimmed || isStreaming) return

    const wsClient = (window as any).__wsClient
    if (!wsClient) return

    const sessionId = activeSessionId || 'default'
    wsClient.send('chat.send', { text: trimmed, sessionId })

    useChatStore.getState().addMessage({
      id: crypto.randomUUID(),
      role: 'user',
      content: trimmed,
      timestamp: Date.now(),
    })

    setText('')
    textareaRef.current?.focus()
  }

  const handleCancel = () => {
    const wsClient = (window as any).__wsClient
    if (wsClient) wsClient.send('chat.cancel', {})
  }

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      if (isStreaming) {
        handleCancel()
      } else {
        handleSubmit()
      }
    }
  }

  return (
    <div className="border-t border-border bg-surface p-3">
      <div className="flex gap-2 items-end">
        <textarea
          ref={textareaRef}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="输入消息... (Shift+Enter 换行, Enter 发送)"
          rows={1}
          className="flex-1 bg-surface-light border border-border rounded px-3 py-2 text-sm text-text placeholder-text-darker resize-none focus:outline-none focus:border-border-focus transition-colors scrollbar-thin"
          style={{ maxHeight: '120px' }}
        />
        <button
          onClick={isStreaming ? handleCancel : handleSubmit}
          className={`p-2 rounded transition-colors ${
            isStreaming
              ? 'bg-error/20 text-error hover:bg-error/30'
              : 'bg-primary text-white hover:bg-primary-light'
          }`}
          title={isStreaming ? '取消生成' : '发送'}
        >
          {isStreaming ? <Square size={16} /> : <Send size={16} />}
        </button>
      </div>
    </div>
  )
}
```

- [ ] **Step 2: 验证编译**

```bash
cd frontend && npx tsc --noEmit
```

Expected: 无类型错误

- [ ] **Step 3: Commit**

```bash
git add frontend/src/components/input/
git commit -m "feat: add InputPanel with native textarea (IME support)"
```

---

### Task 14: 会话列表组件 (SessionList, SessionItem)

**Files:**
- Create: `frontend/src/components/session/SessionList.tsx`
- Create: `frontend/src/components/session/SessionItem.tsx`

- [ ] **Step 1: 创建 `SessionItem.tsx`**

```tsx
import { MessageSquare, Trash2 } from 'lucide-react'
import type { Session } from '@/types'
import { useSessionStore } from '@/stores/sessionStore'

interface SessionItemProps {
  session: Session
  isActive: boolean
}

export default function SessionItem({ session, isActive }: SessionItemProps) {
  const setActiveSession = useSessionStore((s) => s.setActiveSession)
  const deleteSession = useSessionStore((s) => s.deleteSession)

  return (
    <div
      onClick={() => setActiveSession(session.id)}
      className={`group flex items-center gap-2 px-3 py-2 cursor-pointer text-sm transition-colors ${
        isActive
          ? 'bg-selection-bg text-selection-fg border-l-2 border-primary'
          : 'text-text-dim hover:bg-surface-light border-l-2 border-transparent'
      }`}
    >
      <MessageSquare size={14} className="flex-shrink-0" />
      <span className="flex-1 truncate">{session.title}</span>
      <button
        onClick={(e) => {
          e.stopPropagation()
          deleteSession(session.id)
        }}
        className="opacity-0 group-hover:opacity-100 p-1 rounded hover:bg-surface-lighter text-text-darker hover:text-error transition-all"
        title="删除会话"
      >
        <Trash2 size={12} />
      </button>
    </div>
  )
}
```

- [ ] **Step 2: 创建 `SessionList.tsx`**

```tsx
import { useSessionStore } from '@/stores/sessionStore'
import SessionItem from './SessionItem'
import { useEffect } from 'react'

export default function SessionList() {
  const sessions = useSessionStore((s) => s.sessions)
  const activeSessionId = useSessionStore((s) => s.activeSessionId)
  const loadSessions = useSessionStore((s) => s.loadSessions)
  const isLoading = useSessionStore((s) => s.isLoading)

  useEffect(() => {
    loadSessions()
  }, [])

  if (isLoading) {
    return (
      <div className="p-4 text-text-darker text-sm text-center">
        加载中...
      </div>
    )
  }

  if (sessions.length === 0) {
    return (
      <div className="p-4 text-text-darker text-sm text-center">
        暂无会话，点击 + 新建
      </div>
    )
  }

  return (
    <div className="py-1">
      {sessions.map((session) => (
        <SessionItem
          key={session.id}
          session={session}
          isActive={session.id === activeSessionId}
        />
      ))}
    </div>
  )
}
```

- [ ] **Step 3: 验证编译**

```bash
cd frontend && npx tsc --noEmit
```

Expected: 无类型错误

- [ ] **Step 4: Commit**

```bash
git add frontend/src/components/session/
git commit -m "feat: add session list components (SessionList, SessionItem)"
```

---

### Task 15: 工具调用卡片组件 (ToolCallCard)

**Files:**
- Create: `frontend/src/components/chat/ToolCallCard.tsx`

- [ ] **Step 1: 创建 `ToolCallCard.tsx`**

```tsx
import { useState } from 'react'
import { Wrench, ChevronDown, ChevronRight, Loader2, CheckCircle2, XCircle } from 'lucide-react'
import type { ToolCall } from '@/types'

interface ToolCallCardProps {
  toolCall: ToolCall
}

export default function ToolCallCard({ toolCall }: ToolCallCardProps) {
  const [argsOpen, setArgsOpen] = useState(false)
  const [resultOpen, setResultOpen] = useState(false)

  const statusIcons = {
    running: <Loader2 size={14} className="text-warning animate-spin" />,
    complete: <CheckCircle2 size={14} className="text-success" />,
    error: <XCircle size={14} className="text-error" />,
  }

  return (
    <div className="mx-4 my-1 border border-border rounded bg-surface-light overflow-hidden">
      <div className="flex items-center gap-2 px-3 py-2 text-sm">
        {statusIcons[toolCall.status]}
        <Wrench size={14} className="text-text-dim" />
        <span className="text-primary-light font-medium">{toolCall.name}</span>
        <span className="text-xs text-text-darker capitalize">{toolCall.status}</span>
      </div>

      <div
        onClick={() => setArgsOpen(!argsOpen)}
        className="flex items-center gap-1 px-3 py-1 text-xs text-text-dim cursor-pointer hover:text-text border-t border-border/50"
      >
        {argsOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        参数
      </div>
      {argsOpen && (
        <pre className="px-6 py-2 text-xs text-text-dim bg-surface overflow-x-auto">
          {JSON.stringify(toolCall.args, null, 2)}
        </pre>
      )}

      {toolCall.result && (
        <>
          <div
            onClick={() => setResultOpen(!resultOpen)}
            className="flex items-center gap-1 px-3 py-1 text-xs text-text-dim cursor-pointer hover:text-text border-t border-border/50"
          >
            {resultOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
            结果
          </div>
          {resultOpen && (
            <pre className="px-6 py-2 text-xs text-text-dim bg-surface overflow-x-auto max-h-40 overflow-y-auto">
              {toolCall.result}
            </pre>
          )}
        </>
      )}
    </div>
  )
}
```

- [ ] **Step 2: 验证编译**

```bash
cd frontend && npx tsc --noEmit
```

Expected: 无类型错误

- [ ] **Step 3: Commit**

```bash
git add frontend/src/components/chat/ToolCallCard.tsx
git commit -m "feat: add ToolCallCard component"
```

---

### Task 16: ChatArea 和 App 组装

**Files:**
- Create: `frontend/src/components/chat/ChatArea.tsx`
- Modify: `frontend/src/App.tsx`
- Modify: `frontend/src/hooks/useWebSocket.ts` — 暴露 wsClient 到 window

- [ ] **Step 1: 创建 `ChatArea.tsx`**

```tsx
import MessageList from './MessageList'
import InputPanel from '@/components/input/InputPanel'

export default function ChatArea() {
  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      <MessageList />
      <InputPanel />
    </div>
  )
}
```

- [ ] **Step 2: 更新 `useWebSocket.ts` 暴露 wsClient**

在 `useEffect` 内部，`client.connect()` 之前添加：

```typescript
;(window as any).__wsClient = client
```

在 cleanup 中添加：

```typescript
;(window as any).__wsClient = null
```

- [ ] **Step 3: 更新 `App.tsx`**

```tsx
import AppLayout from '@/components/layout/AppLayout'
import ChatArea from '@/components/chat/ChatArea'
import { useWebSocket } from '@/hooks/useWebSocket'

function App() {
  useWebSocket()

  return (
    <AppLayout>
      <ChatArea />
    </AppLayout>
  )
}

export default App
```

- [ ] **Step 4: 验证编译**

```bash
cd frontend && npx tsc --noEmit
```

Expected: 无类型错误

- [ ] **Step 5: 验证构建**

```bash
cd frontend && npm run build
```

Expected: 构建成功，输出到 `frontend/dist/`

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/chat/ChatArea.tsx frontend/src/App.tsx frontend/src/hooks/useWebSocket.ts
git commit -m "feat: assemble ChatArea and App with WebSocket integration"
```

---

## 阶段四：集成与测试

### Task 17: 前后端联调验证

- [ ] **Step 1: 启动后端**

```bash
python -m my_tui.api.server
```

Expected: FastAPI 启动在 `http://127.0.0.1:8000`

- [ ] **Step 2: 验证健康检查**

```bash
curl http://127.0.0.1:8000/api/health
```

Expected: `{"status":"ok"}`

- [ ] **Step 3: 验证 WebSocket 连接**

使用浏览器打开 `http://127.0.0.1:8000`，打开开发者工具 Network 标签，确认 WebSocket 连接成功。

- [ ] **Step 4: 验证前端页面加载**

确认页面显示布局（侧边栏 + 聊天区域 + 输入框 + 状态栏），黑红配色正确。

- [ ] **Step 5: 验证中文输入**

在输入框中切换中文输入法，输入中文，确认输入正常无拼音残留。

- [ ] **Step 6: 验证粘贴**

在输入框中粘贴文本，确认粘贴正常。

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "chore: verify frontend-backend integration"
```

---

### Task 18: 最终验证清单

- [ ] 中文 IME 输入正常，无拼音残留
- [ ] 粘贴功能正常
- [ ] 中英文混合输入正常
- [ ] `goat web` 一键启动，自动打开浏览器
- [ ] `goat cli` 命令不受影响
- [ ] 现有 Python 测试全部通过
- [ ] 前端 `npm run build` 构建成功
- [ ] 前端 `npx tsc --noEmit` 无类型错误