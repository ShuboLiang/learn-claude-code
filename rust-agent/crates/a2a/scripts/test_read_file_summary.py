#!/usr/bin/env python3
"""
A2A "读取本地文件并总结" 场景测试

用法:
    python test_read_file_summary.py [文件路径]

默认读取: rust-agent/test_readme.md
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
DEFAULT_FILE = PROJECT_ROOT / "test_readme.md"


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


def send_sync_task(task_id: str, prompt: str) -> tuple[int, dict | str]:
    """发送同步任务"""
    url = urljoin(BASE_URL, "/tasks/send")
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
            # SSE 是流式数据，我们需要逐行读取
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
        print(f"  📊 状态更新: {state} {marker}")
    elif etype == "task-message":
        parts = payload.get("message", {}).get("parts", [])
        for part in parts:
            if part.get("type") == "text":
                text = part.get("text", "")
                # 截断过长的文本
                display = text[:200] + "..." if len(text) > 200 else text
                print(f"  💬 {display}")
    elif etype == "task-artifact":
        name = payload.get("artifact", {}).get("name", "unnamed")
        print(f"  📎 Artifact: {name}")
    else:
        print(f"  📡 {etype}: {str(payload)[:100]}")


def main():
    target_file = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_FILE
    if not target_file.exists():
        print(f"错误: 文件不存在: {target_file}")
        sys.exit(1)

    rel_path = target_file.relative_to(PROJECT_ROOT)
    print("=" * 60)
    print("A2A 读取本地文件并总结 场景测试")
    print(f"目标文件: {rel_path}")
    print(f"服务端地址: {BASE_URL}")
    print("=" * 60)

    # 1. 启动 A2A 服务端
    print("\n[1/4] 启动 A2A 服务端...")
    print(f"      工作目录: {PROJECT_ROOT}")

    # 使用 cargo run 启动，在项目根目录下运行（这样 read_file 能访问到 test_readme.md）
    proc = subprocess.Popen(
        ["cargo", "run", "-p", "rust-agent-a2a", "--quiet"],
        cwd=PROJECT_ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )

    # 2. 等待服务就绪
    print("[2/4] 等待服务端就绪...")
    if not wait_for_server(timeout=120.0):
        print("❌ 服务端启动超时")
        proc.terminate()
        sys.exit(1)
    print("✅ 服务端已就绪")

    # 3. 先验证 Agent Card
    print("\n[3/4] 验证 Agent Card...")
    req = urllib.request.Request(urljoin(BASE_URL, "/.well-known/agent.json"))
    with urllib.request.urlopen(req, timeout=5) as resp:
        card = json.loads(resp.read().decode("utf-8"))
        skills = [s["id"] for s in card.get("skills", [])]
        print(f"      Agent: {card.get('name')}")
        print(f"      Skills: {skills}")

    # 4. 发送"读取并总结"任务
    prompt = f"请读取文件 {rel_path}，并用 3 句话总结其核心内容。"
    print(f"\n[4/4] 发送任务: {prompt}")
    print("-" * 60)

    # 使用流式模式，可以看到实时进度
    code, events = send_stream_task("task-read-summary-001", prompt)

    print("-" * 60)

    # 汇总结果
    print("\n📋 结果汇总:")
    final_status = None
    final_message = ""
    for ev in events:
        if ev.get("event") == "task-status":
            payload = ev.get("payload", {})
            if payload.get("final"):
                final_status = payload.get("status", {}).get("state")
        elif ev.get("event") == "task-message":
            parts = ev.get("payload", {}).get("message", {}).get("parts", [])
            for part in parts:
                if part.get("type") == "text":
                    final_message += part.get("text", "")

    if final_status:
        print(f"   最终状态: {final_status}")
    if final_message:
        print(f"   最终回复:\n   {final_message}")

    # 5. 清理
    print("\n[清理] 关闭服务端...")
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()

    print("✅ 测试完成")


if __name__ == "__main__":
    main()
