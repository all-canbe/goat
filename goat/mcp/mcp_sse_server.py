from __future__ import annotations

import asyncio
import hashlib
import json
import logging
import os
import sys

from .mcp_config import McpConfig, load_mcp_config
from .mcp_tool_defs import build_mcp_tools
from .mcp_dispatch import dispatch_mcp_call, McpDispatchOptions
from .mcp_rate_limit import TokenBucketRateLimiter

logger = logging.getLogger(__name__)


def _cors_headers(origin: str | None, cors_allowlist: set[str] | None = None, preflight: bool = False) -> dict[str, str]:
    headers: dict[str, str] = {}
    if cors_allowlist and origin and origin in cors_allowlist:
        headers["Access-Control-Allow-Origin"] = origin
        headers["Vary"] = "Origin"
        if preflight:
            headers["Access-Control-Allow-Methods"] = "POST, OPTIONS"
            headers["Access-Control-Allow-Headers"] = "Content-Type, Authorization, Accept"
    return headers


async def _read_body_with_cap(body_reader, cap: int) -> str | None:
    chunks: list[bytes] = []
    total = 0
    while True:
        chunk = await body_reader.read(8192)
        if not chunk:
            break
        total += len(chunk)
        if total > cap:
            return None
        chunks.append(chunk)
    return b"".join(chunks).decode("utf-8")


async def start_mcp_sse_server(
    host: str = "127.0.0.1",
    port: int = 8800,
    config: McpConfig | None = None,
):
    try:
        from mcp.server import Server
    except ImportError:
        print("MCP SDK not installed. Run: pip install mcp>=1.10.0", file=sys.stderr)
        sys.exit(1)

    if config is None:
        config = load_mcp_config()

    server = Server(
        {"name": "goat", "version": "0.1.0"},
        {"capabilities": {"tools": {}}},
    )

    @server.list_tools()
    async def handle_list_tools():
        return build_mcp_tools(config.tools_allowlist)

    @server.call_tool()
    async def handle_call_tool(name: str, arguments: dict):
        result = await dispatch_mcp_call(
            tool_name=name,
            params=arguments,
            opts=McpDispatchOptions(remote=True, source_id="sse"),
        )
        return result.content

    cors_allowlist: set[str] = set()
    cors_env = os.environ.get("MCP_CORS_ORIGINS", "")
    if cors_env:
        cors_allowlist = {o.strip() for o in cors_env.split(",") if o.strip()}

    ip_limiter = TokenBucketRateLimiter(
        limit=config.ip_rate_limit,
        window_ms=60_000,
    ) if config.rate_limit_enabled else None

    token_limiter = TokenBucketRateLimiter(
        limit=config.token_rate_limit,
        window_ms=60_000,
    ) if config.rate_limit_enabled else None

    def _hash_token(token: str) -> str:
        return hashlib.sha256(token.encode()).hexdigest()

    valid_token_hash = None
    if config.sse_token:
        valid_token_hash = _hash_token(config.sse_token)

    async def handle_http(scope, receive, send):
        if scope["type"] != "http":
            return

        headers_raw = scope.get("headers", [])
        headers_dict = {k.decode("latin-1").lower(): v.decode("latin-1") for k, v in headers_raw}
        origin = headers_dict.get("origin")

        if scope["method"] == "OPTIONS":
            cors_h = _cors_headers(origin, cors_allowlist, preflight=True)
            await send({
                "type": "http.response.start",
                "status": 204,
                "headers": [(k.encode(), v.encode()) for k, v in cors_h.items()],
            })
            await send({"type": "http.response.body", "body": b""})
            return

        path = scope["path"].rstrip("/")

        client_ip = scope.get("client", ("unknown", 0))[0]

        if ip_limiter:
            allowed_ip, retry = ip_limiter.check(client_ip)
            if not allowed_ip:
                await send({
                    "type": "http.response.start",
                    "status": 429,
                    "headers": [
                        (b"content-type", b"application/json"),
                        (b"retry-after", str(int(retry)).encode()),
                    ],
                })
                await send({"type": "http.response.body", "body": json.dumps({"error": "rate_limited"}).encode()})
                return

        if path == "/sse":
            if scope["method"] == "GET":
                from mcp.server.sse import SseServerTransport
                transport = SseServerTransport("/messages/")

                async def sse_handler(send_sse):
                    async with transport.connect_sse(send_sse) as streams:
                        await server.connect(streams[0], streams[1])

                sse_scope = dict(scope)
                await transport.handle_request(sse_scope, receive, send)
                return

        if path == "/messages":
            if scope["method"] != "POST":
                await send({
                    "type": "http.response.start",
                    "status": 405,
                    "headers": [(b"content-type", b"application/json")],
                })
                await send({"type": "http.response.body", "body": b'{"error":"method_not_allowed"}'})
                return

            body_data = b""
            more_body = True
            while more_body:
                message = await receive()
                if message["type"] == "http.request":
                    body_data += message.get("body", b"")
                    more_body = message.get("more_body", False)
                else:
                    more_body = False

            if len(body_data) > config.body_cap_bytes:
                await send({
                    "type": "http.response.start",
                    "status": 413,
                    "headers": [(b"content-type", b"application/json")],
                })
                await send({"type": "http.response.body", "body": b'{"error":"body_too_large"}'})
                return

            if valid_token_hash:
                auth = headers_dict.get("authorization", "")
                if not auth.startswith("bearer "):
                    await send({
                        "type": "http.response.start",
                        "status": 401,
                        "headers": [(b"content-type", b"application/json")],
                    })
                    await send({"type": "http.response.body", "body": b'{"error":"unauthorized"}'})
                    return
                submitted = auth[7:]
                if _hash_token(submitted) != valid_token_hash:
                    await send({
                        "type": "http.response.start",
                        "status": 401,
                        "headers": [(b"content-type", b"application/json")],
                    })
                    await send({"type": "http.response.body", "body": b'{"error":"invalid_token"}'})
                    return
                token_id = submitted[:8]
                if token_limiter:
                    allowed_token, _ = token_limiter.check(token_id)
                    if not allowed_token:
                        await send({
                            "type": "http.response.start",
                            "status": 429,
                            "headers": [(b"content-type", b"application/json")],
                        })
                        await send({"type": "http.response.body", "body": b'{"error":"token_rate_limited"}'})
                        return

            from mcp.server.sse import SseServerTransport
            transport = SseServerTransport("/messages/")

            async def handle_receive_body():
                yield body_data
                while True:
                    yield b""

            await transport.handle_post_message(scope, receive, send)

            return

        if path == "/health":
            await send({
                "type": "http.response.start",
                "status": 200,
                "headers": [(b"content-type", b"application/json")],
            })
            await send({"type": "http.response.body", "body": b'{"status":"ok"}'})
            return

        await send({
            "type": "http.response.start",
            "status": 404,
            "headers": [(b"content-type", b"application/json")],
        })
        await send({"type": "http.response.body", "body": b'{"error":"not_found"}'})

    import uvicorn

    config_uvicorn = uvicorn.Config(
        app=handle_http,
        host=host,
        port=port,
        log_level="info",
        lifespan="off",
    )
    server_instance = uvicorn.Server(config_uvicorn)

    logger.info(f"MCP SSE server listening on http://{host}:{port}")
    print(f"MCP SSE server listening on http://{host}:{port}", file=sys.stderr)

    await server_instance.serve()


def _main():
    import argparse

    parser = argparse.ArgumentParser(description="Goat MCP SSE Server")
    parser.add_argument("--host", type=str, default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8800)
    args = parser.parse_args()

    asyncio.run(start_mcp_sse_server(host=args.host, port=args.port))


if __name__ == "__main__":
    _main()