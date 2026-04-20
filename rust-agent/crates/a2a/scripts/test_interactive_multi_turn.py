#!/usr/bin/env python3
"""
A2A 交互式多轮对话测试脚本

用法:
    python test_interactive_multi_turn.py

交互流程:
    1. 输入初始任务
    2. Agent 流式回复
    3. 输入下一条消息继续对话（或 exit 退出）
    4. 支持任意轮数
"""

import json
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from urllib.parse import urljoin

BASE_URL = "http://localhost:3001"
A2A_CARGO_PATH = Path(__file__).parent.parent
PROJECT_ROOT = A2A_CARGO_PATH.parent.parent


def wait_for_server(timeout: float = 60.0) -> bool:
    url = urljoin(BASE_URL, "/.well-known/agent.json")
    start = time.time()
    while time.time() - start < timeout:
        try:
            req = urllib.request.Request(url, method="GET")
            with urllib.request.urlopen(req, timeout=2) as resp:
                if resp.status == 200:
                    return True
        except urllib.error.URLError:
            pass
        time.sleep(0.5)
    return False


def send_stream_task(task_id: str, prompt: str) -> tuple[int, list[dict]]:
    url = urljoin(BASE_URL, "/tasks/sendSubscribe")
    payload = {
        "id": task_id,
        "message": {
            "role": "user",
            "parts": [{"type": "text", "text": prompt}]
        }
    }
    body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
    req = urllib.request.Request(
        url,
        method="POST",
        data=body,
        headers={"Content-Type": "application/json", "Accept": "text/event-stream"}
    )

    events = []
    try:
        with urllib.request.urlopen(req, timeout=300) as resp:
            buffer = b""
            while True:
                chunk = resp.read(1024)
                if not chunk:
                    break
                buffer += chunk
                while b"\n\n" in buffer:
                    frame, buffer = buffer.split(b"\n\n", 1)
                    event = parse_sse_frame(frame.decode("utf-8"))
                    if event:
                        events.append(event)
                        print_sse_event(event)
    except Exception as e:
        print(f"[SSE] 流读取异常: {e}")
    return 200, events


def send_followup_task(task_id: str, prompt: str) -> tuple[int, dict | str]:
    url = urljoin(BASE_URL, f"/tasks/{task_id}/send")
    payload = {
        "id": task_id,
        "message": {
            "role": "user",
            "parts": [{"type": "text", "text": prompt}]
        }
    }
    body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
    req = urllib.request.Request(
        url,
        method="POST",
        data=body,
        headers={"Content-Type": "application/json", "Accept": "application/json"}
    )
    try:
        with urllib.request.urlopen(req, timeout=300) as resp:
            resp_body = resp.read().decode("utf-8")
            return resp.status, json.loads(resp_body)
    except urllib.error.HTTPError as e:
        resp_body = e.read().decode("utf-8")
        try:
            return e.code, json.loads(resp_body)
        except json.JSONDecodeError:
            return e.code, resp_body
    except Exception as e:
        return -1, str(e)


def parse_sse_frame(frame: str) -> dict | None:
    event_type = ""
    data = ""
    for line in frame.strip().split("\n"):
        if line.startswith("event:"):
            event_type = line[6:].strip()
        elif line.startswith("data:"):
            data += line[5:].strip()
    if not event_type or not data:
        return None
    try:
        payload = json.loads(data)
        return {"event": event_type, "payload": payload}
    except json.JSONDecodeError:
        return {"event": event_type, "raw": data}


def print_sse_event(event: dict):
    etype = event.get("event", "unknown")
    payload = event.get("payload", {})

    if etype == "task-status":
        state = payload.get("status", {}).get("state", "?")
        is_final = payload.get("final", False)
        marker = "[最终]" if is_final else ""
        print(f"      📊 状态: {state} {marker}")
    elif etype == "task-message":
        parts = payload.get("message", {}).get("parts", [])
        for part in parts:
            if part.get("type") == "text":
                text = part.get("text", "")
                display = text[:500] + "..." if len(text) > 500 else text
                print(f"      💬 {display}")
    elif etype == "task-artifact":
        name = payload.get("artifact", {}).get("name", "unnamed")
        print(f"      📎 Artifact: {name}")


def extract_agent_text(response: dict) -> str:
    history = response.get("history", [])
    for msg in history:
        if msg.get("role") == "agent":
            parts = msg.get("parts", [])
            for part in parts:
                if part.get("type") == "text":
                    return part.get("text", "")
    return ""


def read_user_input(prompt_text: str) -> str | None:
    try:
        user = input(prompt_text).strip()
    except (EOFError, KeyboardInterrupt):
        print()
        return None
    return user


def main():
    print("=" * 60)
    print("A2A 交互式多轮对话测试")
    print(f"工作目录: {PROJECT_ROOT}")
    print(f"服务端地址: {BASE_URL}")
    print("提示: 输入 exit / quit / q 结束对话")
    print("=" * 60)

    # 启动服务端
    print("\n[启动] 启动 A2A 服务端...")
    proc = subprocess.Popen(
        ["cargo", "run", "-p", "rust-agent-a2a", "--quiet"],
        cwd=PROJECT_ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )

    print("[等待] 等待服务端就绪...")
    if not wait_for_server(timeout=120.0):
        print("❌ 服务端启动超时")
        proc.terminate()
        sys.exit(1)
    print("✅ 服务端已就绪")

    # Agent Card
    req = urllib.request.Request(urljoin(BASE_URL, "/.well-known/agent.json"))
    with urllib.request.urlopen(req, timeout=5) as resp:
        card = json.loads(resp.read().decode("utf-8"))
        print(f"\n🤖 Agent: {card.get('name')}")
        print(f"   Skills: {[s['id'] for s in card.get('skills', [])]}")

    # 获取 task_id
    task_id_input = read_user_input("\n请输入任务 ID（直接回车使用默认）: ")
    task_id = task_id_input if task_id_input else "task-interactive-001"
    print(f"   使用任务 ID: {task_id}")

    # 第一轮：用户输入初始 prompt
    first_prompt = read_user_input("\n👤 你: ")
    if first_prompt is None or first_prompt.lower() in ("exit", "quit", "q"):
        print("👋 再见")
        proc.terminate()
        return

    print(f"\n{'─' * 60}")
    print("🚀 Round 1 — 流式发送")
    print("─" * 60)

    code, _ = send_stream_task(task_id, first_prompt)
    if code != 200:
        print(f"❌ 初始任务发送失败: {code}")
        proc.terminate()
        return

    time.sleep(0.5)

    # 获取第一轮 Agent 回复
    req = urllib.request.Request(urljoin(BASE_URL, f"/tasks/{task_id}"))
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            task_data = json.loads(resp.read().decode("utf-8"))
            agent_text = extract_agent_text(task_data)
    except Exception:
        agent_text = ""

    if agent_text:
        print(f"\n🤖 Agent:\n{agent_text.strip()}")

    # 多轮循环
    round_num = 2
    while True:
        user_input = read_user_input(f"\n👤 你（Round {round_num}）: ")
        if user_input is None:
            break
        if user_input.lower() in ("exit", "quit", "q"):
            print("👋 结束对话")
            break
        if not user_input:
            continue

        print(f"\n{'─' * 60}")
        print(f"🚀 Round {round_num} — follow-up")
        print("─" * 60)

        code, body = send_followup_task(task_id, user_input)
        if code != 200:
            print(f"❌ 请求失败: {code} - {body}")
            break
        if isinstance(body, dict) and "error" in body:
            print(f"❌ 服务端错误: {body['error']}")
            break

        agent_text = extract_agent_text(body)
        if agent_text:
            print(f"\n🤖 Agent:\n{agent_text.strip()}")
        else:
            print("\n🤖 Agent: （无文本回复）")

        round_num += 1

    # 清理
    print("\n[清理] 关闭服务端...")
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
    print("✅ 测试完成")


if __name__ == "__main__":
    main()
