"""HTTP API 客户端 + SSE 流式解析."""

import json
from dataclasses import dataclass
from typing import Any

import httpx


@dataclass
class ServerConfig:
    base_url: str
    session_id: str


@dataclass
class ServerEvent:
    event: str
    data: dict


@dataclass
class BotInfo:
    name: str
    nickname: str
    role: str
    description: str


@dataclass
class SessionSummary:
    id: str
    created_at: str
    last_active: str
    message_count: int
    preview: str


_config: ServerConfig | None = None
_client: httpx.AsyncClient | None = None


def init(base_url: str, session_id: str) -> None:
    global _config, _client
    _config = ServerConfig(base_url=base_url, session_id=session_id)
    _client = httpx.AsyncClient()


def get_config() -> ServerConfig:
    if _config is None:
        raise RuntimeError("API 未初始化")
    return _config


def set_session_id(session_id: str) -> None:
    global _config
    if _config is None:
        raise RuntimeError("API 未初始化")
    _config = ServerConfig(_config.base_url, session_id)


async def close() -> None:
    global _client
    if _client is not None:
        await _client.aclose()
        _client = None


async def create_session() -> tuple[str, str]:
    cfg = get_config()
    resp = await _client.post(f"{cfg.base_url}/sessions")
    resp.raise_for_status()
    data = resp.json()
    sid = data.get("id")
    if not sid:
        raise RuntimeError("创建会话失败: 服务器未返回会话 ID")
    set_session_id(sid)
    return sid, data.get("model", "unknown")


async def clear_session() -> None:
    cfg = get_config()
    resp = await _client.post(f"{cfg.base_url}/sessions/{cfg.session_id}/clear")
    resp.raise_for_status()


async def _parse_sse(response: httpx.Response):
    """逐行解析 SSE 流，yield ServerEvent."""
    current_event = ""
    async for line in response.aiter_lines():
        if line is None:
            continue
        if line.startswith("event:"):
            current_event = line[6:].lstrip() if line[6:7] == " " else line[6:]
        elif line.startswith("data:"):
            data = line[5:].lstrip() if line[5:6] == " " else line[5:]
            if data == "[DONE]":
                return
            try:
                parsed = json.loads(data)
                yield ServerEvent(event=current_event, data=parsed)
                if current_event == "done":
                    return
            except json.JSONDecodeError:
                pass
            current_event = ""
        elif line == "":
            # SSE 事件之间有空行，重置 current_event
            current_event = ""


async def send_message(content: str):
    """流式发送消息，返回 SSE 事件异步生成器."""
    cfg = get_config()
    async with _client.stream(
        "POST",
        f"{cfg.base_url}/sessions/{cfg.session_id}/messages",
        json={"content": content},
        timeout=None,
    ) as response:
        response.raise_for_status()
        async for event in _parse_sse(response):
            yield event


async def send_bot_task(bot_name: str, content: str):
    """向指定 Bot 委派任务，返回 SSE 事件异步生成器."""
    cfg = get_config()
    async with _client.stream(
        "POST",
        f"{cfg.base_url}/bots/{bot_name}/task",
        json={"content": content},
        timeout=None,
    ) as response:
        response.raise_for_status()
        async for event in _parse_sse(response):
            yield event


async def fetch_bots() -> list[BotInfo]:
    cfg = get_config()
    resp = await _client.get(f"{cfg.base_url}/bots")
    resp.raise_for_status()
    data = resp.json()
    return [BotInfo(**b) for b in data.get("bots", [])]


async def fetch_sessions() -> list[SessionSummary]:
    cfg = get_config()
    resp = await _client.get(f"{cfg.base_url}/sessions")
    resp.raise_for_status()
    data = resp.json()
    return [SessionSummary(**s) for s in data.get("sessions", [])]


async def fetch_session_messages(sid: str) -> list[dict[str, Any]]:
    cfg = get_config()
    resp = await _client.get(f"{cfg.base_url}/sessions/{sid}/messages")
    resp.raise_for_status()
    data = resp.json()
    return data.get("messages", [])
