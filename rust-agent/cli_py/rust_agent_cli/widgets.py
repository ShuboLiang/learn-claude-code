"""自定义 Textual widgets."""

from textual.widgets import Static, Input, Markdown
from textual.containers import VerticalScroll


class MessageUser(Static):
    DEFAULT_CSS = """
    MessageUser {
        color: cyan;
        text-style: bold;
        padding: 0 1;
    }
    """

    def __init__(self, content: str) -> None:
        super().__init__(f"你: {content}", markup=False)


class MessageAssistant(Markdown):
    DEFAULT_CSS = """
    MessageAssistant {
        padding: 0 1;
    }
    """

    def __init__(self, content: str) -> None:
        super().__init__(content)


class MessageAssistantStream(Static):
    DEFAULT_CSS = """
    MessageAssistantStream {
        padding: 0 1;
    }
    """

    def __init__(self) -> None:
        super().__init__("", markup=False)


class MessageThinking(Static):
    DEFAULT_CSS = """
    MessageThinking {
        color: gray;
        text-style: dim;
        padding: 0 1;
    }
    """

    def __init__(self, content: str) -> None:
        text = content if len(content) <= 200 else content[:200] + "..."
        super().__init__(f"💭 {text}", markup=False)

    def update_content(self, content: str) -> None:
        text = content if len(content) <= 200 else content[:200] + "..."
        self.update(f"💭 {text}")


class MessageToolCall(Static):
    DEFAULT_CSS = """
    MessageToolCall {
        color: yellow;
        padding: 0 1;
    }
    """

    def __init__(self, content: str) -> None:
        name, desc = self._format(content)
        super().__init__(f"┌─ 🔧 {name}\n│ {desc}", markup=False)

    @staticmethod
    def _format(content: str) -> tuple[str, str]:
        import json
        try:
            data = json.loads(content)
            name = data.get("name", "unknown")
            inp = data.get("input", {})
            desc = (
                inp.get("command")
                or inp.get("path")
                or inp.get("query")
                or inp.get("content")
                or json.dumps(inp)
            )
            return name, str(desc)
        except json.JSONDecodeError:
            return "tool", content


class MessageToolResult(Static):
    DEFAULT_CSS = """
    MessageToolResult {
        text-style: dim;
        padding: 0 1;
    }
    """

    def __init__(self, content: str) -> None:
        max_show = 5000
        if len(content) > max_show:
            content = content[:max_show] + f"\n... [还有 {len(content) - max_show} 字符未显示]"
        super().__init__(f"└─ {content}", markup=False)


class MessageBotCall(Static):
    DEFAULT_CSS = """
    MessageBotCall {
        color: magenta;
        text-style: bold;
        padding: 0 1;
    }
    """

    def __init__(self, content: str) -> None:
        super().__init__(f"╭─ 🤖 {content}", markup=False)


class MessageSystem(Static):
    DEFAULT_CSS = """
    MessageSystem {
        text-style: dim;
        padding: 0 1;
    }
    """

    def __init__(self, content: str) -> None:
        super().__init__(content, markup=False)


class ChatLog(VerticalScroll):
    DEFAULT_CSS = """
    ChatLog {
        height: 1fr;
        border: none;
        padding: 0;
    }
    """

    def __init__(self, *args, **kwargs) -> None:
        super().__init__(*args, **kwargs)
        self._stream_widget: MessageAssistantStream | None = None
        self._thinking_widget: MessageThinking | None = None
        self._retry_widget: MessageSystem | None = None

    def _is_at_bottom(self) -> bool:
        return self.scroll_y >= self.max_scroll_y - 1

    def _maybe_scroll_end(self, was_at_bottom: bool) -> None:
        if was_at_bottom:
            self.scroll_end(animate=False)

    def add_message(self, widget: Static | Markdown) -> None:
        was_at_bottom = self._is_at_bottom()
        self.mount(widget)
        self._maybe_scroll_end(was_at_bottom)

    def set_streaming(self, text: str) -> None:
        was_at_bottom = self._is_at_bottom()
        if self._stream_widget is None:
            self._stream_widget = MessageAssistantStream()
            self.mount(self._stream_widget)
        self._stream_widget.update(text)
        self._maybe_scroll_end(was_at_bottom)

    def remove_stream_widget(self) -> None:
        if self._stream_widget is not None:
            self._stream_widget.remove()
            self._stream_widget = None

    def set_thinking(self, text: str) -> None:
        was_at_bottom = self._is_at_bottom()
        if self._thinking_widget is None:
            self._thinking_widget = MessageThinking(text)
            self.mount(self._thinking_widget)
        else:
            self._thinking_widget.update_content(text)
        self._maybe_scroll_end(was_at_bottom)

    def remove_thinking_widget(self) -> None:
        if self._thinking_widget is not None:
            self._thinking_widget.remove()
            self._thinking_widget = None

    def set_retry(self, text: str) -> None:
        was_at_bottom = self._is_at_bottom()
        if self._retry_widget is None:
            self._retry_widget = MessageSystem(text)
            self.mount(self._retry_widget)
        else:
            self._retry_widget.update(text)
        self._maybe_scroll_end(was_at_bottom)

    def clear_retry(self) -> None:
        if self._retry_widget is not None:
            self._retry_widget.remove()
            self._retry_widget = None

    def clear_messages(self) -> None:
        self.remove_children()
        self._stream_widget = None
        self._thinking_widget = None
        self._retry_widget = None


class CommandInput(Input):
    DEFAULT_CSS = """
    CommandInput {
        border: solid $primary;
        padding: 0 1;
    }
    """

    def __init__(self, *args, **kwargs) -> None:
        super().__init__(*args, **kwargs)
        self.placeholder = "输入消息, Enter 提交..."
        self.history: list[str] = []
        self._history_index = -1
        self.is_multiline = False
        self._buffer = ""

    def enter_multiline(self) -> None:
        self.is_multiline = True
        self._buffer = ""
        self.placeholder = "多行模式: 输入 /send 提交, ESC 或 /cancel 取消..."
        self.value = ""

    def exit_multiline(self) -> None:
        self.is_multiline = False
        self._buffer = ""
        self.value = ""
        self.placeholder = "输入消息, Enter 提交..."

    def add_history(self, text: str) -> None:
        if text.strip():
            self.history.append(text)
        self._history_index = -1

    def on_key(self, event) -> None:
        key = event.key
        if self.is_multiline:
            if key == "escape":
                self.exit_multiline()
                event.stop()
            return

        if key == "up":
            event.stop()
            if not self.history:
                return
            if self._history_index == -1:
                self._history_index = len(self.history) - 1
            elif self._history_index > 0:
                self._history_index -= 1
            self.value = self.history[self._history_index]
        elif key == "down":
            event.stop()
            if not self.history or self._history_index == -1:
                return
            if self._history_index < len(self.history) - 1:
                self._history_index += 1
                self.value = self.history[self._history_index]
            else:
                self._history_index = -1
                self.value = ""
        elif key == "escape":
            event.stop()
            self.value = ""
            self._history_index = -1
