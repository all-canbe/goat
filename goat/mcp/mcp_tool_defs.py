from __future__ import annotations

import inspect
import json
from typing import Any

from langchain_core.tools import BaseTool
from pydantic import BaseModel

from ..tools.tools import BUILTIN_TOOLS

_TYPE_MAP: dict[str, str] = {
    "string": "string",
    "integer": "integer",
    "number": "number",
    "boolean": "boolean",
    "array": "array",
    "object": "object",
    "null": "null",
}

_PYDANTIC_TYPE_MAP: dict[type, str] = {
    str: "string",
    int: "number",
    float: "number",
    bool: "boolean",
    list: "array",
    dict: "object",
}


def _pydantic_model_to_json_schema(model: type[BaseModel]) -> dict:
    result: dict[str, Any] = {"type": "object", "properties": {}}
    required: list[str] = []

    for field_name, field_info in model.model_fields.items():
        is_required = field_info.is_required()
        if is_required:
            required.append(field_name)

        prop: dict[str, Any] = {}
        annotation = field_info.annotation
        orig_annotation = field_info.annotation

        for origin in getattr(annotation, "__metadata__", ()):
            pass

        if annotation is not None:
            origin = getattr(annotation, "__origin__", None)
            args = getattr(annotation, "__args__", ())

            if origin is list:
                prop["type"] = "array"
                if args and args[0] is not type(None):
                    item_type = _pydantic_type_to_json_type(args[0])
                    if item_type == "nested":
                        prop["items"] = _pydantic_model_to_json_schema(args[0])
                    else:
                        prop["items"] = {"type": item_type}
            elif origin is dict:
                prop["type"] = "object"
            elif origin is type(None):  # noqa: E721
                attr = getattr(annotation, "__args__", ())
                non_none = [a for a in attr if a is not type(None)]
                if non_none:
                    inner = _pydantic_type_to_json_type(non_none[0])
                    if inner == "nested":
                        nested_schema = _pydantic_model_to_json_schema(non_none[0])
                        prop.update(nested_schema)
                    else:
                        prop["type"] = inner
            elif annotation in _PYDANTIC_TYPE_MAP:
                prop["type"] = _PYDANTIC_TYPE_MAP[annotation]
            elif isinstance(annotation, type) and issubclass(annotation, BaseModel):
                nested = _pydantic_model_to_json_schema(annotation)
                prop.update(nested)
            else:
                prop["type"] = "string"
        else:
            prop["type"] = "string"

        if field_info.description:
            prop["description"] = field_info.description
        if field_info.default is not None and field_info.default is not ... and not is_required:
            prop["default"] = field_info.default

        result["properties"][field_name] = prop

    if required:
        result["required"] = required

    return result


def _pydantic_type_to_json_type(py_type: type) -> str:
    if py_type in _PYDANTIC_TYPE_MAP:
        return _PYDANTIC_TYPE_MAP[py_type]
    if isinstance(py_type, type) and issubclass(py_type, BaseModel):
        return "nested"
    return "string"


TOOL_SCHEMA_OVERRIDES: dict[str, dict] = {}


def langchain_tool_to_mcp_schema(tool: BaseTool) -> dict:
    if tool.name in TOOL_SCHEMA_OVERRIDES:
        return TOOL_SCHEMA_OVERRIDES[tool.name]

    schema: dict[str, Any] = {
        "name": tool.name,
        "description": tool.description or "",
    }

    if tool.args_schema and hasattr(tool.args_schema, "model_fields"):
        schema["inputSchema"] = _pydantic_model_to_json_schema(tool.args_schema)
    else:
        params: dict[str, Any] = {}
        sig = inspect.signature(tool._run)
        for param_name, param in sig.parameters.items():
            if param_name == "self":
                continue
            p: dict[str, Any] = {"type": "string"}
            if param.annotation is not inspect.Parameter.empty:
                py_type = param.annotation
                p["type"] = _PYDANTIC_TYPE_MAP.get(py_type, "string")
            params[param_name] = p

        schema["inputSchema"] = {
            "type": "object",
            "properties": params,
            "required": [],
        }

    return schema


def build_mcp_tools(tool_names: list[str] | None = None) -> list[dict]:
    if tool_names is not None:
        tools = [
            langchain_tool_to_mcp_schema(BUILTIN_TOOLS[n])
            for n in tool_names
            if n in BUILTIN_TOOLS
        ]
    else:
        tools = [
            langchain_tool_to_mcp_schema(t) for t in BUILTIN_TOOLS.values()
        ]
    return tools


def params_to_json_schema(params_def: dict[str, Any]) -> dict:
    properties: dict[str, Any] = {}
    required: list[str] = []

    for key, def_ in params_def.items():
        prop: dict[str, Any] = {}
        ptype = def_.get("type", "string")
        prop["type"] = _TYPE_MAP.get(ptype, "string")
        if def_.get("description"):
            prop["description"] = def_["description"]
        if def_.get("required"):
            required.append(key)
        properties[key] = prop

    result: dict[str, Any] = {"type": "object", "properties": properties}
    if required:
        result["required"] = required
    return result