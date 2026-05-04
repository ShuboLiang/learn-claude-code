"""Textual App — 主应用 + 事件路由."""

import asyncio
from pathlib import Path

from textual.app import App, ComposeResult
from textual.reactive import reactive
from textual.widgets import Static
from textual import work
import re

from rust_agent_cli import api_client as api
from rust_agent_cli.session_utils import transform_messages
from rust_agent_cli.widgets import (
    ChatLog,
    CommandInput,
    MessageUser,
    MessageAssistant,
    MessageThinking,
    MessageToolCall,
    MessageToolResult,
    MessageBotCall,
    MessageSystem,
)


class RustAgentApp(App):
    CSS_PATH = str(Path(__file__).parent / "styles.tcss")

    session_id = reactive("")
    model = reactive("")
    is_loading = reactive(False)
    error_msg = reactive("")
    bots = reactive(list)
    session_list = reactive(list)

    def __init__(self, server_port: int, server_process) -> None:
        super().__init__()
        self.server_port = server_port
        self.server_process = server_process
        self._current_reply_text = ""
        self._current_thinking_text = ""
        self._stream_done = False
        self._active_bot_name: str | None = None

    def compose(self) -> ComposeResult:
        yield ChatLog(id="chat-log")
        yield Static(id="multiline-preview")
        yield CommandInput(id="input")
        yield Static(id="error-bar")

    async def on_mount(self) -> None:
        api.init(f"http://127.0.0.1:{self.server_port}", "")
        try:
            self.bots = await api.fetch_bots()
        except Exception:
            self.bots = []
        self.query_one("#input", CommandInput).focus()

    async def on_unmount(self) -> None:
        await api.close()

    # ── Reactive watchers ──

    def watch_error_msg(self, msg: str) -> None:
        bar = self.query_one("#error-bar", Static)
        if msg:
            bar.update(f"Error: {msg}")
            bar.styles.display = "block"
        else:
            bar.update("")
            bar.styles.display = "none"

    def watch_is_loading(self, loading: bool) -> None:
        inp = self.query_one("#input", CommandInput)
        if loading:
            inp.placeholder = "(等待响应中...)"
        elif self.model:
            inp.placeholder = f"[{self.model}] 输入消息, Enter 提交, /@bot 委派任务, /m 多行..."
        else:
            inp.placeholder = "输入消息, Enter 提交, /@bot 委派任务, /m 多行..."

    def watch_model(self, model: str) -> None:
        if not self.is_loading:
            self.watch_is_loading(False)

    # ── Input handling ──

    async def on_input_submitted(self, event) -> None:
        inp = self.query_one("#input", CommandInput)
        raw = event.value
        text = raw.strip()

        if inp.is_multiline:
            lower = text.lower()
            if lower == "/send":
                content = inp._buffer
                inp.exit_multiline()
                preview = self.query_one("#multiline-preview", Static)
                preview.update("")
                preview.styles.display = "none"
                if content.strip():
                    await self._handle_user_input(content)
            elif lower == "/cancel":
                inp.exit_multiline()
                preview = self.query_one("#multiline-preview", Static)
                preview.update("")
                preview.styles.display = "none"
            else:
                inp._buffer = inp._buffer + "\n" + raw if inp._buffer else raw
                preview = self.query_one("#multiline-preview", Static)
                preview.update(f"已输入内容预览:\n{inp._buffer}")
                preview.styles.display = "block"
                inp.value = ""
            return

        inp.value = ""

        if not text:
            return

        lower = text.lower()
        if lower in ("q", "quit", "exit", "/exit"):
            self.exit()
            return
        if lower == "/clear":
            await self._do_clear()
            return
        if lower in ("/m", "/multiline"):
            inp.enter_multiline()
            return
        if lower == "/bots":
            self._do_bots()
            return
        if lower == "/sessions":
            await self._do_sessions()
            return
        if lower.startswith("/load "):
            await self._do_load(text[6:].strip())
            return

        inp.add_history(text)
        # 支持 \n 转义为真实换行
        await self._handle_user_input(text.replace("\\n", "\n"))

    async def _handle_user_input(self, text: str) -> None:
        if self.is_loading:
            return

        chat = self.query_one("#chat-log", ChatLog)
        actual = text
        self._active_bot_name = None

        bot_match = re.match(r"^/@(\S+)\s+(.*)", text.strip())
        if bot_match:
            bot_name = bot_match.group(1)
            task = bot_match.group(2)
            display = next((b.nickname for b in self.bots if b.name == bot_name), bot_name)
            chat.add_message(MessageBotCall(f"@{display}: {task}"))
            actual = f'请使用 call_bot 工具，bot 名称为 "{bot_name}"，执行以下任务：\n\n{task}'
            self._active_bot_name = display

        bot_match2 = re.match(r"^/@@(\S+)\s+(.*)", text.strip())
        if bot_match2:
            bot_name = bot_match2.group(1)
            task = bot_match2.group(2)
            display = next((b.nickname for b in self.bots if b.name == bot_name), bot_name)
            chat.add_message(MessageBotCall(f"@{display}: {task}"))
            await self._handle_bot_task(bot_name, task)
            return

        chat.add_message(MessageUser(text))
        await self._send_message(actual)

    # ── Commands ──

    async def _do_clear(self) -> None:
        chat = self.query_one("#chat-log", ChatLog)
        chat.add_message(MessageSystem("═══ 以上对话已被清除 ═══"))
        if api.get_config().session_id:
            try:
                await api.clear_session()
            except Exception:
                pass

    def _do_bots(self) -> None:
        chat = self.query_one("#chat-log", ChatLog)
        if not self.bots:
            chat.add_message(MessageSystem("没有可用的 Bot。请在 bots/ 目录下创建 BOT.md 文件。"))
            return
        lines = []
        for b in self.bots:
            nick = f" ({b.nickname})" if b.nickname else ""
            desc = f": {b.description}" if b.description else ""
            lines.append(f"  @{b.name}{nick} - {b.role}{desc}")
        chat.add_message(MessageSystem(f"可用 Subagent:\n{'\n'.join(lines)}\n\n使用 /@@botname 任务描述 来委派任务"))

    async def _do_sessions(self) -> None:
        chat = self.query_one("#chat-log", ChatLog)
        try:
            sessions = await api.fetch_sessions()
            self.session_list = sessions
            if not sessions:
                chat.add_message(MessageSystem("═══ 暂无历史会话 ═══"))
                return
            lines = []
            for i, s in enumerate(sessions, 1):
                from datetime import datetime
                try:
                    dt = datetime.fromisoformat(s.last_active.replace("Z", "+00:00"))
                    date = dt.strftime("%m-%d %H:%M")
                except Exception:
                    date = s.last_active[:16]
                lines.append(f"[{i}] {date}  ({s.message_count} 条)  {s.preview}")
            chat.add_message(
                MessageSystem(
                    f"═══ 历史会话 ═══\n{'\n'.join(lines)}\n════════════════\n使用 /load <序号> 或 /load <uuid> 恢复"
                )
            )
        except Exception as e:
            chat.add_message(MessageSystem(f"获取历史会话失败: {e}"))

    async def _do_load(self, arg: str) -> None:
        chat = self.query_one("#chat-log", ChatLog)
        if self.is_loading:
            chat.add_message(MessageSystem("当前有进行中的对话，请等待结束后再加载。"))
            return
        target_id = None
        try:
            idx = int(arg)
            if 1 <= idx <= len(self.session_list):
                target_id = self.session_list[idx - 1].id
        except ValueError:
            target_id = arg
        if not target_id:
            chat.add_message(MessageSystem("无效序号或 UUID，先用 /sessions 查看列表。"))
            return
        try:
            msgs = await api.fetch_session_messages(target_id)
            new_msgs = transform_messages(msgs)
            api.set_session_id(target_id)
            self.session_id = target_id
            preview = next((s.preview for s in self.session_list if s.id == target_id), target_id[:8])
            chat.clear_messages()
            for m in new_msgs:
                self._add_message_widget(m.role, m.content)
            chat.add_message(MessageSystem(f"═══ 已恢复会话 {preview} ═══"))
        except Exception as e:
            chat.add_message(MessageSystem(f"加载会话失败: {e}"))

    def _add_message_widget(self, role: str, content: str) -> None:
        chat = self.query_one("#chat-log", ChatLog)
        if role == "user":
            chat.add_message(MessageUser(content))
        elif role == "assistant":
            chat.add_message(MessageAssistant(content))
        elif role == "thinking":
            chat.add_message(MessageThinking(content))
        elif role == "tool_call":
            chat.add_message(MessageToolCall(content))
        elif role == "tool_result":
            chat.add_message(MessageToolResult(content))
        elif role == "bot_call":
            chat.add_message(MessageBotCall(content))
        elif role == "system":
            chat.add_message(MessageSystem(content))

    # ── Message sending ──

    async def _send_message(self, content: str) -> None:
        if not self.session_id:
            try:
                sid, model = await api.create_session()
                self.session_id = sid
                self.model = model
            except Exception as e:
                self.error_msg = f"会话创建失败: {e}"
                return
        self.error_msg = ""
        self.is_loading = True
        self._current_reply_text = ""
        self._current_thinking_text = ""
        self._stream_done = False
        self._run_stream(content)

    async def _handle_bot_task(self, bot_name: str, task: str) -> None:
        if not task.strip():
            self.error_msg = f"@{bot_name}: 请提供任务描述"
            return
        display = next((b.nickname for b in self.bots if b.name == bot_name), bot_name)
        self._active_bot_name = display
        self.is_loading = True
        self._current_reply_text = ""
        self._current_thinking_text = ""
        self._stream_done = False
        self._run_stream(task, is_bot_task=True, bot_name=bot_name)

    # ── Stream worker ──

    @work(group="stream", exclusive=True)
    async def _run_stream(self, content: str, is_bot_task: bool = False, bot_name: str | None = None) -> None:
        try:
            if is_bot_task:
                stream = api.send_bot_task(bot_name, content)
            else:
                stream = api.send_message(content)
            async for event in stream:
                if self._stream_done:
                    break
                if event.event == "done":
                    self._stream_done = True
                    self._on_done()
                    break
                self._on_sse_event(event)
        except asyncio.CancelledError:
            # 用户主动取消
            if not self._stream_done:
                self._on_stream_cancelled()
            raise
        except Exception as e:
            if not self._stream_done:
                self._on_stream_error(str(e))

    def _on_sse_event(self, event: api.ServerEvent) -> None:
        chat = self.query_one("#chat-log", ChatLog)
        ev = event.event
        data = event.data

        if ev == "thinking_delta":
            self._current_thinking_text += data.get("content", "")
            chat.set_thinking(self._current_thinking_text)

        elif ev == "text_delta":
            if self._current_thinking_text:
                chat.add_message(MessageThinking(self._current_thinking_text))
                self._current_thinking_text = ""
                chat.remove_thinking_widget()
            self._current_reply_text += data.get("content", "")
            chat.set_streaming(self._current_reply_text)

        elif ev == "tool_call":
            if self._current_reply_text:
                chat.add_message(MessageAssistant(self._current_reply_text))
                self._current_reply_text = ""
                chat.remove_stream_widget()
            if data.get("name") == "call_bot":
                bn = data.get("input", {}).get("name", "unknown")
                task = data.get("input", {}).get("task", "")
                chat.add_message(MessageBotCall(f"@{bn}: {task}"))
            else:
                import json
                chat.add_message(MessageToolCall(json.dumps({"name": data.get("name"), "input": data.get("input")})))

        elif ev == "tool_result":
            chat.add_message(MessageToolResult(data.get("output", "")))

        elif ev == "turn_end":
            chat.clear_retry()
            if self._current_reply_text:
                chat.add_message(MessageAssistant(self._current_reply_text))
                self._current_reply_text = ""
                chat.remove_stream_widget()
            api_calls = data.get("api_calls")
            token_usage = data.get("token_usage")
            info = self._format_turn_end(api_calls, token_usage)
            chat.add_message(MessageSystem(info))
            self.is_loading = False
            self._active_bot_name = None

        elif ev == "retrying":
            detail = data.get("detail", "")
            wait = data.get("wait_seconds", 0)
            attempt = data.get("attempt", 0) + 1
            max_retries = data.get("max_retries", 0) + 1
            chat.set_retry(f"⏳ 正在重试 ({attempt}/{max_retries}) — {detail}，等待 {wait}s")

        elif ev == "error":
            self.error_msg = data.get("message", "未知错误")
            chat.clear_retry()

    def _format_turn_end(self, api_calls, token_usage) -> str:
        info = "── 完成"
        if api_calls is not None:
            info = f"── 完成，API 调用 {api_calls} 次"
        if token_usage:
            inp = self._fmt_tokens(token_usage.get("input_tokens", 0))
            out = self._fmt_tokens(token_usage.get("output_tokens", 0))
            cache = token_usage.get("cache_read_tokens", 0)
            info += f" │ Token: {inp}入/{out}出"
            if cache > 0:
                info += f" (缓存命中 {self._fmt_tokens(cache)})"
        if self._active_bot_name:
            info = info.replace("── 完成", f"── @{self._active_bot_name} 完成")
        info += " ──"
        return info

    @staticmethod
    def _fmt_tokens(n: int) -> str:
        if n >= 1_000_000:
            return f"{n / 1_000_000:.1f}".replace(".0", "") + "m"
        if n >= 1_000:
            return f"{n / 1_000:.1f}".replace(".0", "") + "k"
        return str(n)

    def _on_done(self) -> None:
        chat = self.query_one("#chat-log", ChatLog)
        if self._current_reply_text:
            chat.add_message(MessageAssistant(self._current_reply_text))
            self._current_reply_text = ""
            chat.remove_stream_widget()
        if self._current_thinking_text:
            chat.add_message(MessageThinking(self._current_thinking_text))
            self._current_thinking_text = ""
            chat.remove_thinking_widget()
        chat.clear_retry()
        self.is_loading = False
        self._active_bot_name = None
        self.workers.cancel_group(self, "stream")

    def _on_stream_cancelled(self) -> None:
        chat = self.query_one("#chat-log", ChatLog)
        if self._current_reply_text:
            chat.add_message(MessageAssistant(self._current_reply_text))
            self._current_reply_text = ""
            chat.remove_stream_widget()
        if self._current_thinking_text:
            chat.add_message(MessageThinking(self._current_thinking_text))
            self._current_thinking_text = ""
            chat.remove_thinking_widget()
        chat.clear_retry()
        chat.add_message(MessageSystem("── 已中断 ──"))
        self.is_loading = False
        self._active_bot_name = None

    def _on_stream_error(self, msg: str) -> None:
        chat = self.query_one("#chat-log", ChatLog)
        if self._current_reply_text:
            chat.add_message(MessageAssistant(self._current_reply_text))
            self._current_reply_text = ""
            chat.remove_stream_widget()
        if self._current_thinking_text:
            chat.add_message(MessageThinking(self._current_thinking_text))
            self._current_thinking_text = ""
            chat.remove_thinking_widget()
        chat.clear_retry()
        # 忽略 "terminated" 错误（服务器断开连接）
        if "terminated" not in msg.lower():
            self.error_msg = msg
        chat.add_message(MessageSystem("── 连接已断开 ──"))
        self.is_loading = False
        self._active_bot_name = None

    # ── Key handling ──

    def on_key(self, event) -> None:
        if event.key == "escape":
            if self.is_loading:
                self.workers.cancel_group(self, "stream")
                event.stop()
                return
            inp = self.query_one("#input", CommandInput)
            if inp.is_multiline:
                inp.exit_multiline()
                preview = self.query_one("#multiline-preview", Static)
                preview.update("")
                preview.styles.display = "none"
                event.stop()
                return
            inp.value = ""
            inp._history_index = -1
            event.stop()
