"""消息转换工具 — 复刻 cli/src/session-utils.ts."""

from dataclasses import dataclass
from typing import Any


@dataclass
class Message:
    role: str
    content: str


def transform_messages(api_messages: list[dict[str, Any]]) -> list[Message]:
    result: list[Message] = []
    for msg in api_messages:
        role = msg.get("role")
        content = msg.get("content")
        if role == "user":
            if isinstance(content, str):
                result.append(Message(role="user", content=content))
            elif isinstance(content, list):
                texts: list[str] = []
                for block in content:
                    if block.get("type") == "tool_result":
                        if texts:
                            result.append(Message(role="user", content="".join(texts)))
                            texts = []
                        result.append(
                            Message(role="tool_result", content=str(block.get("content", "")))
                        )
                    elif block.get("type") == "text" and isinstance(block.get("text"), str):
                        texts.append(block["text"])
                if texts:
                    result.append(Message(role="user", content="".join(texts)))
        elif role == "assistant":
            if isinstance(content, str):
                result.append(Message(role="assistant", content=content))
            elif isinstance(content, list):
                for block in content:
                    if block.get("type") == "text" and isinstance(block.get("text"), str):
                        result.append(Message(role="assistant", content=block["text"]))
                    elif block.get("type") == "tool_use":
                        result.append(
                            Message(
                                role="tool_call",
                                content=__import__("json").dumps(
                                    {"name": block.get("name"), "input": block.get("input")}
                                ),
                            )
                        )
    return result
