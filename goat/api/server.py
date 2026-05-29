from __future__ import annotations

import asyncio
import json
import webbrowser
from pathlib import Path

import uvicorn
from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles

from goat.api.routes import router
from goat.api.websocket import ws_manager, create_event_bridge
from goat.api.chat_handler import web_chat_handler


def create_app(workspace: str | None = None) -> FastAPI:
    app = FastAPI(title="Goat API", version="0.1.0")

    @app.on_event("startup")
    async def on_startup():
        await web_chat_handler.initialize(workspace=workspace)
        ws_manager.set_session_manager(web_chat_handler)
        create_event_bridge(web_chat_handler.event_bus)

    app.add_middleware(
        CORSMiddleware,
        allow_origins=["*"],
        allow_credentials=True,
        allow_methods=["*"],
        allow_headers=["*"],
    )

    app.include_router(router)

    @app.websocket("/ws")
    async def websocket_endpoint(websocket: WebSocket):
        session_id = websocket.query_params.get("session_id", "default")
        await ws_manager.connect(websocket, session_id)
        try:
            while True:
                raw = await websocket.receive_text()
                try:
                    data = json.loads(raw)
                except json.JSONDecodeError:
                    continue
                data.setdefault("session_id", session_id)
                await ws_manager.handle_message(websocket, data)
        except WebSocketDisconnect:
            ws_manager.disconnect(websocket, session_id)

    frontend_dist = Path(__file__).resolve().parent.parent.parent / "frontend" / "dist"
    if frontend_dist.exists():
        app.mount("/", StaticFiles(directory=str(frontend_dist), html=True), name="static")

    return app


def start_server(host: str = "127.0.0.1", port: int = 8000, open_browser: bool = False, workspace: str | None = None) -> None:
    app = create_app(workspace=workspace)

    if open_browser:
        webbrowser.open(f"http://{host}:{port}")

    uvicorn.run(app, host=host, port=port, log_level="info")


if __name__ == "__main__":
    start_server()