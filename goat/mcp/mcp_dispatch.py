from __future__ import annotations

import asyncio
import json
import traceback
from dataclasses import dataclass, field
from typing import Any, Callable

from langchain_core.tools import BaseTool

from ..tools.tools import BUILTIN_TOOLS, describe_tool_action
from ..core.event_bus import EventBus, EventType


@dataclass
class McpDispatchOptions:
    remote: bool = True
    source_id: str = "default"
    meta_hook: Callable | None = None
    event_bus: EventBus | None = None
    approval_system: Any | None = None


@dataclass
class McpToolResult:
    content: list[dict]
    isError: bool = False
    _meta: dict = field(default_factory=dict)


def validate_mcp_params(tool_name: str, params: dict) -> str | None:
    tool = BUILTIN_TOOLS.get(tool_name)
    if tool is None:
        return f"Unknown tool: {tool_name}"

    if tool.args_schema and hasattr(tool.args_schema, "model_fields"):
        schema = tool.args_schema
        for field_name, field_info in schema.model_fields.items():
            if field_info.is_required():
                if params.get(field_name) is None:
                    return f"Missing required parameter: {field_name}"

            value = params.get(field_name)
            if value is not None:
                annotation = field_info.annotation
                if annotation is str and not isinstance(value, str):
                    return f'Parameter "{field_name}" must be a string'
                if annotation is int and not isinstance(value, (int, float)):
                    return f'Parameter "{field_name}" must be a number'
                if annotation is float and not isinstance(value, (int, float)):
                    return f'Parameter "{field_name}" must be a number'
                if annotation is bool and not isinstance(value, bool):
                    return f'Parameter "{field_name}" must be a boolean'
                if annotation is list and not isinstance(value, list):
                    return f'Parameter "{field_name}" must be an array'
                if annotation is dict and not isinstance(value, dict):
                    return f'Parameter "{field_name}" must be an object'

    return None


def _summarize_mcp_params(tool_name: str, params: dict) -> dict:
    tool = BUILTIN_TOOLS.get(tool_name)
    if tool is None or not params:
        return {"tool": tool_name}

    known_keys: set[str] = set()
    if tool.args_schema and hasattr(tool.args_schema, "model_fields"):
        known_keys = set(tool.args_schema.model_fields.keys())

    provided_names = set(params.keys())
    declared = provided_names & known_keys
    unknown_count = len(provided_names - known_keys)

    body_str = json.dumps(params, ensure_ascii=False)
    approx_kb = (len(body_str.encode("utf-8")) + 1023) // 1024

    return {
        "tool": tool_name,
        "declared_keys": sorted(declared),
        "unknown_key_count": unknown_count,
        "approx_kb": approx_kb,
    }


async def dispatch_mcp_call(
    tool_name: str,
    params: dict,
    opts: McpDispatchOptions | None = None,
) -> McpToolResult:
    if opts is None:
        opts = McpDispatchOptions()

    tool = BUILTIN_TOOLS.get(tool_name)
    if tool is None:
        return McpToolResult(
            content=[{"type": "text", "text": json.dumps({"error": "tool_not_found", "message": f"Unknown tool: {tool_name}"})}],
            isError=True,
        )

    validation_error = validate_mcp_params(tool_name, params)
    if validation_error:
        return McpToolResult(
            content=[{"type": "text", "text": json.dumps({"error": "invalid_params", "message": validation_error})}],
            isError=True,
        )

    if opts.approval_system and opts.remote:
        try:
            from ..security.approval import Decision
            evaluate = getattr(opts.approval_system, 'request_tool_approval', None)
            if callable(evaluate):
                approval_result = await evaluate(
                    agent_id='mcp',
                    tool_name=tool_name,
                    tool_args=params,
                )
                if approval_result.decision != Decision.ALLOW:
                    return McpToolResult(
                        content=[{"type": "text", "text": json.dumps({"error": "approval_denied", "message": f"Tool '{tool_name}' was not approved: {approval_result.message}"})}],
                        isError=True,
                    )
        except Exception:
            pass

    if opts.event_bus and opts.remote:
        opts.event_bus.publish_nowait(
            source_id=opts.source_id,
            event_type=EventType.TOOL_CALL,
            payload=json.dumps({"tool_name": tool_name, "args": params, "description": describe_tool_action(tool_name, params)}),
            agent_name=f"mcp:{tool_name}",
        )

    try:
        if asyncio.iscoroutinefunction(tool._arun):
            result = await tool._arun(**params)
        elif hasattr(tool, "ainvoke"):
            result = await tool.ainvoke(params)
        elif hasattr(tool, "_arun"):
            if asyncio.iscoroutinefunction(tool._arun):
                result = await tool._arun(**params)
            else:
                result = tool._arun(**params)
        elif hasattr(tool, "func") and tool.func:
            result = tool.func(**params)
        else:
            result = tool._run(**params)
    except Exception as e:
        error_data = {
            "error": "tool_execution_error",
            "message": str(e),
            "tool": tool_name,
        }
        if opts.event_bus:
            opts.event_bus.publish_nowait(
                source_id=opts.source_id,
                event_type=EventType.TOOL_RESULT,
                payload=json.dumps({"tool_name": tool_name, "result": str(e), "error": True}),
                agent_name=f"mcp:{tool_name}",
            )
        return McpToolResult(
            content=[{"type": "text", "text": json.dumps(error_data)}],
            isError=True,
        )

    result_text = result if isinstance(result, str) else json.dumps(result, ensure_ascii=False)
    out = McpToolResult(
        content=[{"type": "text", "text": result_text}],
    )

    if opts.event_bus:
        opts.event_bus.publish_nowait(
            source_id=opts.source_id,
            event_type=EventType.TOOL_RESULT,
            payload=json.dumps({"tool_name": tool_name, "result": result_text[:500]}),
            agent_name=f"mcp:{tool_name}",
        )

    if opts.meta_hook:
        try:
            meta = await opts.meta_hook(tool_name, result_text)
            if meta and isinstance(meta, dict):
                out._meta = meta
        except Exception:
            pass

    return out