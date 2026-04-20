#!/usr/bin/env python3
"""
A2A 会议室预定 — 自动化多轮对话测试

用法:
    python test_meeting_room_booking.py

测试流程（自动执行，无需人工输入）:
    Round 1: 请使用会议预定接口定个会议室
    Round 2: 提供会议时间、人数、时长、主题
    （如 Agent 仍需更多信息，可继续扩展 DIALOGUE_SCRIPT）
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
A2A_CARGO_PATH = Path(__file__).parent.parent  # crates/a2a
PROJECT_ROOT = A2A_CARGO_PATH.parent.parent  # rust-agent

# 自动对话剧本：每轮的用户输入
DIALOGUE_SCRIPT = [
    "请使用会议预定接口定个会议室",
    "2026-04-25 14:30，8人，2小时，项目例会",
]

TASK_ID = "task-auto-meeting-001"


def wait_for_server(timeout: float = 60.0) -> bool:
    """轮询等待服务端就绪"""
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
    """发送流式任务，实时打印 SSE 事件"""
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
    """向已有任务发送 follow-up 消息"""
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
    """解析单个 SSE 帧"""
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
    """美观地打印 SSE 事件"""
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
                display = text[:400] + "..." if len(text) > 400 else text
                print(f"      💬 {display}")
    elif etype == "task-artifact":
        name = payload.get("artifact", {}).get("name", "unnamed")
        print(f"      📎 Artifact: {name}")


def extract_agent_text(response: dict) -> str:
    """从响应中提取 Agent 文本回复"""
    history = response.get("history", [])
    for msg in history:
        if msg.get("role") == "agent":
            parts = msg.get("parts", [])
            for part in parts:
                if part.get("type") == "text":
                    return part.get("text", "")
    return ""


def run_round(round_num: int, user_input: str, is_first: bool = False) -> tuple[bool, str]:
    """
    执行单轮对话。
    第一轮使用 SSE 流式，后续使用同步 follow-up。
    返回 (success, agent_text)
    """
    print(f"\n{'─' * 60}")
    print(f"🔄 Round {round_num}")
    print(f"👤 User: {user_input}")
    print("─" * 60)

    if is_first:
        code, _ = send_stream_task(TASK_ID, user_input)
        if code != 200:
            print(f"❌ 流式任务发送失败: {code}")
            return False, ""
        # 流结束后稍等，让服务端把 context 落盘
        time.sleep(0.5)
        # 查询最终任务状态获取完整文本
        req = urllib.request.Request(urljoin(BASE_URL, f"/tasks/{TASK_ID}"))
        try:
            with urllib.request.urlopen(req, timeout=10) as resp:
                task_data = json.loads(resp.read().decode("utf-8"))
                agent_text = extract_agent_text(task_data)
        except Exception as e:
            print(f"⚠️  查询任务状态失败: {e}")
            agent_text = ""
    else:
        code, body = send_followup_task(TASK_ID, user_input)
        if code != 200:
            print(f"❌ Follow-up 失败: {code} - {body}")
            return False, ""
        if isinstance(body, dict) and "error" in body:
            print(f"❌ 服务端返回错误: {body['error']}")
            return False, ""
        agent_text = extract_agent_text(body)

    if agent_text:
        print(f"\n🤖 Agent:\n{agent_text.strip()}")
    else:
        print("\n🤖 Agent: （无文本回复）")

    return True, agent_text


def main():
    print("=" * 60)
    print("A2A 会议室预定 — 自动化多轮对话测试")
    print(f"工作目录: {PROJECT_ROOT}")
    print(f"服务端地址: {BASE_URL}")
    print(f"对话轮数: {len(DIALOGUE_SCRIPT)}")
    print("=" * 60)

    # 1. 启动服务端
    print("\n[1/3] 启动 A2A 服务端...")
    proc = subprocess.Popen(
        ["cargo", "run", "-p", "rust-agent-a2a", "--quiet"],
        cwd=PROJECT_ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )

    # 2. 等待就绪
    print("[2/3] 等待服务端就绪...")
    if not wait_for_server(timeout=120.0):
        print("❌ 服务端启动超时")
        proc.terminate()
        sys.exit(1)
    print("✅ 服务端已就绪")

    # 3. 验证 Agent Card
    print("\n[3/3] 验证 Agent Card...")
    req = urllib.request.Request(urljoin(BASE_URL, "/.well-known/agent.json"))
    with urllib.request.urlopen(req, timeout=5) as resp:
        card = json.loads(resp.read().decode("utf-8"))
        skills = [s["id"] for s in card.get("skills", [])]
        print(f"      Agent: {card.get('name')}")
        print(f"      Skills: {skills}")

    # 4. 执行自动对话剧本
    print("\n" + "=" * 60)
    print("开始执行自动对话剧本")
    print("=" * 60)

    all_agent_replies = []
    for i, user_input in enumerate(DIALOGUE_SCRIPT, start=1):
        ok, agent_text = run_round(i, user_input, is_first=(i == 1))
        if not ok:
            print("\n❌ 对话中断")
            break
        all_agent_replies.append((i, agent_text))

    # 5. 汇总
    print("\n" + "=" * 60)
    print("📋 对话汇总")
    print("=" * 60)
    for rnd, text in all_agent_replies:
        summary = text[:200] + "..." if len(text) > 200 else text
        print(f"  Round {rnd}: {summary}")

    # 6. 清理
    print("\n[清理] 关闭服务端...")
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()

    print("✅ 测试完成")


if __name__ == "__main__":
    main()
