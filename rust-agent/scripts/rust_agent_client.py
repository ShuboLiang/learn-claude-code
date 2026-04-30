#!/usr/bin/env python3
"""rust-agent-server (/core/server) 的 Python 客户端

功能覆盖:
- 健康检查
- 会话管理 (创建 / 查询 / 删除 / 清空)
- 流式消息发送 (SSE)
- OpenAI 兼容 /v1/chat/completions
- Bot 列表与 Bot 任务委派 (SSE)

依赖:
    pip install requests

用法示例见文件底部 __main__。
"""

from __future__ import annotations

import json
import uuid
from dataclasses import dataclass, field
from typing import Any, Callable, Dict, Generator, Iterable, List, Optional

import requests


# ---------------------------------------------------------------------------
# 数据模型
# ---------------------------------------------------------------------------


@dataclass
class SessionInfo:
    id: str
    model: str
    created_at: str
    message_count: int = 0
    last_active: Optional[str] = None


@dataclass
class BotInfo:
    name: str
    nickname: str = ""
    role: str = ""
    description: str = ""


@dataclass
class SseEvent:
    """SSE 事件包装器"""

    event: str  # text_delta, tool_call, tool_result, turn_end, done, error
    data: Dict[str, Any]

    # 便捷属性
    @property
    def is_text(self) -> bool:
        return self.event == "text_delta"

    @property
    def is_tool_call(self) -> bool:
        return self.event == "tool_call"

    @property
    def is_tool_result(self) -> bool:
        return self.event == "tool_result"

    @property
    def is_turn_end(self) -> bool:
        return self.event == "turn_end"

    @property
    def is_done(self) -> bool:
        return self.event == "done"

    @property
    def is_error(self) -> bool:
        return self.event == "error"

    @property
    def text(self) -> str:
        """如果是 text_delta，返回 content"""
        return self.data.get("content", "") if self.is_text else ""


@dataclass
class ChatCompletionResult:
    id: str
    model: str
    content: Optional[str]
    tool_calls: List[Dict[str, Any]] = field(default_factory=list)
    finish_reason: str = "stop"
    usage: Dict[str, int] = field(default_factory=dict)
    raw: Dict[str, Any] = field(default_factory=dict, repr=False)


# ---------------------------------------------------------------------------
# 客户端
# ---------------------------------------------------------------------------


class RustAgentClient:
    def __init__(self, base_url: str = "http://localhost:3000", timeout: int = 120):
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout
        self._session = requests.Session()

    # --- 内部辅助 ---

    def _url(self, path: str) -> str:
        return f"{self.base_url}{path}"

    def _request(
        self,
        method: str,
        path: str,
        json_data: Optional[Dict[str, Any]] = None,
        stream: bool = False,
        headers: Optional[Dict[str, str]] = None,
    ) -> requests.Response:
        url = self._url(path)
        default_headers = {
            "Content-Type": "application/json",
            "Accept": "application/json",
        }
        if headers:
            default_headers.update(headers)
        if stream:
            default_headers["Accept"] = "text/event-stream"

        resp = self._session.request(
            method,
            url,
            json=json_data,
            headers=default_headers,
            timeout=self.timeout,
            stream=stream,
        )
        resp.raise_for_status()
        return resp

    @staticmethod
    def _tap_lines(
        lines: Iterable[str],
        callback: Callable[[str], None],
    ) -> Generator[str, None, None]:
        """在迭代过程中对每一行执行回调，再原样 yield。"""
        for line in lines:
            callback(line)
            yield line

    @staticmethod
    def _parse_sse_lines(lines: Iterable[str]) -> Generator[SseEvent, None, None]:
        """解析 SSE 文本流为 SseEvent 生成器"""
        event_name = "message"
        data_parts: List[str] = []

        for line in lines:
            line = line.rstrip("\n\r")
            if line.startswith("event:"):
                event_name = line[len("event:") :].strip()
            elif line.startswith("data:"):
                data_parts.append(line[len("data:") :].strip())
            elif line == "":
                if data_parts:
                    try:
                        payload = json.loads("\n".join(data_parts))
                    except json.JSONDecodeError:
                        payload = {"raw": "\n".join(data_parts)}
                    yield SseEvent(event=event_name, data=payload)
                event_name = "message"
                data_parts = []

        # 处理最后未以空行结尾的数据
        if data_parts:
            try:
                payload = json.loads("\n".join(data_parts))
            except json.JSONDecodeError:
                payload = {"raw": "\n".join(data_parts)}
            yield SseEvent(event=event_name, data=payload)

    # --- 健康检查 ---

    def health(self) -> Dict[str, Any]:
        resp = self._request("GET", "/")
        return resp.json()

    # --- 会话管理 ---

    def create_session(self) -> SessionInfo:
        resp = self._request("POST", "/sessions")
        data = resp.json()
        return SessionInfo(
            id=data["id"],
            model=data.get("model", ""),
            created_at=data["created_at"],
        )

    def get_session(self, session_id: str) -> SessionInfo:
        resp = self._request("GET", f"/sessions/{session_id}")
        data = resp.json()
        return SessionInfo(
            id=data["id"],
            model="",
            created_at=data["created_at"],
            message_count=data.get("message_count", 0),
            last_active=data.get("last_active"),
        )

    def delete_session(self, session_id: str) -> bool:
        resp = self._request("DELETE", f"/sessions/{session_id}")
        return resp.status_code == 204

    def clear_session(self, session_id: str) -> Dict[str, Any]:
        resp = self._request("POST", f"/sessions/{session_id}/clear")
        return resp.json()

    # --- 消息发送 (SSE 流式) ---

    def send_message_stream_raw(
        self,
        session_id: str,
        content: str,
    ) -> Generator[str, None, None]:
        """向指定会话发送消息，以 SSE 流式接收原始文本行。

        Yields:
            str: 每条原始 SSE 文本行（如 ``event:text_delta``、``data:{...}``、空行等）
        """
        resp = self._request(
            "POST",
            f"/sessions/{session_id}/messages",
            json_data={"content": content},
            stream=True,
        )
        try:
            for line in resp.iter_lines(decode_unicode=True):
                if line is not None:
                    yield line
        finally:
            resp.close()

    def send_message_stream(
        self,
        session_id: str,
        content: str,
        on_event: Optional[Callable[[SseEvent], None]] = None,
        on_raw_line: Optional[Callable[[str], None]] = None,
    ) -> Generator[SseEvent, None, None]:
        """向指定会话发送消息，以 SSE 流式接收 Agent 事件。

        Args:
            on_event: 每次解析出 SSE 事件后的回调
            on_raw_line: 每条原始 SSE 行到达时的回调（可用于打印原始流）

        Yields:
            SseEvent: 每个 SSE 事件对象
        """
        raw = self.send_message_stream_raw(session_id, content)
        if on_raw_line:
            raw = self._tap_lines(raw, on_raw_line)
        for event in self._parse_sse_lines(raw):
            if on_event:
                on_event(event)
            yield event

    def send_message(
        self,
        session_id: str,
        content: str,
        on_event: Optional[Callable[[SseEvent], None]] = None,
        on_raw_line: Optional[Callable[[str], None]] = None,
    ) -> str:
        """发送消息并收集所有文本增量，返回最终完整文本。"""
        full_text = ""
        for event in self.send_message_stream(
            session_id, content, on_event=on_event, on_raw_line=on_raw_line
        ):
            if event.is_text:
                full_text += event.text
            elif event.is_error:
                raise RustAgentError(
                    event.data.get("code", "unknown"), event.data.get("message", "")
                )
            elif event.is_done:
                break
        return full_text

    # --- OpenAI 兼容端点 ---

    def chat_completion(
        self,
        messages: List[Dict[str, Any]],
        model: Optional[str] = None,
        tools: Optional[List[Dict[str, Any]]] = None,
        max_tokens: Optional[int] = None,
    ) -> ChatCompletionResult:
        """调用 /v1/chat/completions (非流式)"""
        payload: Dict[str, Any] = {"messages": messages}
        if model is not None:
            payload["model"] = model
        if tools is not None:
            payload["tools"] = tools
        if max_tokens is not None:
            payload["max_tokens"] = max_tokens

        resp = self._request("POST", "/v1/chat/completions", json_data=payload)
        data = resp.json()
        choice = data.get("choices", [{}])[0]
        msg = choice.get("message", {})
        return ChatCompletionResult(
            id=data.get("id", ""),
            model=data.get("model", ""),
            content=msg.get("content"),
            tool_calls=msg.get("tool_calls") or [],
            finish_reason=choice.get("finish_reason", "stop"),
            usage=data.get("usage", {}),
            raw=data,
        )

    # --- Bot 管理 ---

    def list_bots(self) -> List[BotInfo]:
        resp = self._request("GET", "/bots")
        data = resp.json()
        bots: List[BotInfo] = []
        for b in data.get("bots", []):
            bots.append(
                BotInfo(
                    name=b.get("name", ""),
                    nickname=b.get("nickname", ""),
                    role=b.get("role", ""),
                    description=b.get("description", ""),
                )
            )
        return bots

    def run_bot_task_stream_raw(
        self,
        bot_name: str,
        content: str,
    ) -> Generator[str, None, None]:
        """向指定 Bot 委派任务，SSE 流式接收原始文本行。

        Yields:
            str: 每条原始 SSE 文本行
        """
        resp = self._request(
            "POST",
            f"/bots/{bot_name}/task",
            json_data={"content": content},
            stream=True,
        )
        try:
            for line in resp.iter_lines(decode_unicode=True):
                if line is not None:
                    yield line
        finally:
            resp.close()

    def run_bot_task_stream(
        self,
        bot_name: str,
        content: str,
        on_event: Optional[Callable[[SseEvent], None]] = None,
        on_raw_line: Optional[Callable[[str], None]] = None,
    ) -> Generator[SseEvent, None, None]:
        """向指定 Bot 委派任务，SSE 流式接收结果。"""
        raw = self.run_bot_task_stream_raw(bot_name, content)
        if on_raw_line:
            raw = self._tap_lines(raw, on_raw_line)
        for event in self._parse_sse_lines(raw):
            if on_event:
                on_event(event)
            yield event

    def run_bot_task(
        self,
        bot_name: str,
        content: str,
        on_event: Optional[Callable[[SseEvent], None]] = None,
        on_raw_line: Optional[Callable[[str], None]] = None,
    ) -> str:
        """向 Bot 委派任务并收集完整文本回复。"""
        full_text = ""
        for event in self.run_bot_task_stream(
            bot_name, content, on_event=on_event, on_raw_line=on_raw_line
        ):
            if event.is_text:
                full_text += event.text
            elif event.is_error:
                raise RustAgentError(
                    event.data.get("code", "unknown"), event.data.get("message", "")
                )
            elif event.is_done:
                break
        return full_text


# ---------------------------------------------------------------------------
# 异常
# ---------------------------------------------------------------------------


class RustAgentError(Exception):
    def __init__(self, code: str, message: str):
        self.code = code
        self.message = message
        super().__init__(f"[{code}] {message}")


# ---------------------------------------------------------------------------
# 示例用法
# ---------------------------------------------------------------------------


def _demo():
    client = RustAgentClient(base_url="http://localhost:3000")

    # 1. 健康检查
    print("=== Health ===")
    print(client.health())

    # 2. 创建会话
    print("\n=== Create Session ===")
    sess = client.create_session()
    print(f"Session ID: {sess.id}, Model: {sess.model}")

    # 3. 流式发送消息（带回调）
    print("\n=== Send Message (stream) ===")

    def on_event(ev: SseEvent):
        if ev.is_text:
            print(ev.text, end="", flush=True)
        elif ev.is_tool_call:
            print(f"\n[Tool Call] {ev.data.get('name')} => {ev.data.get('input')}")
        elif ev.is_tool_result:
            print(f"\n[Tool Result] {ev.data.get('name')} => {ev.data.get('output')}")
        elif ev.is_turn_end:
            usage = ev.data.get("token_usage")
            calls = ev.data.get("api_calls", 0)
            print(f"\n[Turn End] API calls: {calls}, usage: {usage}")
        elif ev.is_error:
            print(f"\n[Error] {ev.data}")

    def on_raw_line(line: str):
        # 让流式传输过程透明：实时打印原始 SSE 行
        if line:
            print(f"[RAW] {line}")
        else:
            print("[RAW] <empty line>")

    try:
        reply = client.send_message(
            sess.id,
            "[mock:429]请用 bash 运行 echo hello-from-python-client",
            on_event=on_event,
            on_raw_line=on_raw_line,
        )
        print(f"\n\nFinal text:\n{reply}")
    except RustAgentError as e:
        print(f"Agent error: {e}")

    # 4. 查询会话状态
    print("\n=== Get Session ===")
    info = client.get_session(sess.id)
    print(f"Messages: {info.message_count}, Last active: {info.last_active}")

    # 5. 清空会话
    print("\n=== Clear Session ===")
    print(client.clear_session(sess.id))

    # 6. OpenAI 兼容端点
    print("\n=== Chat Completion ===")
    try:
        result = client.chat_completion(
            messages=[
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "Say hello in Python."},
            ]
        )
        print(f"Model: {result.model}, Finish reason: {result.finish_reason}")
        print(f"Content: {result.content}")
    except requests.HTTPError as e:
        print(f"HTTP Error: {e}")

    # 7. Bot 列表
    print("\n=== List Bots ===")
    bots = client.list_bots()
    for b in bots:
        print(f"  - {b.name} ({b.nickname or 'no nickname'} / {b.role or 'no role'})")

    # 8. Bot 任务（如果有 Bot）
    if bots:
        print(f"\n=== Bot Task ({bots[0].name}) ===")
        try:
            bot_reply = client.run_bot_task(
                bots[0].name,
                "请简单介绍一下你自己",
                on_event=on_event,
                on_raw_line=on_raw_line,
            )
            print(f"\nBot reply:\n{bot_reply}")
        except RustAgentError as e:
            print(f"Bot error: {e}")

    # 9. 删除会话
    print("\n=== Delete Session ===")
    client.delete_session(sess.id)
    print("Deleted.")


if __name__ == "__main__":
    _demo()
