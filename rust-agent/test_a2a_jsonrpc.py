#!/usr/bin/env python3
"""测试 rust-agent-a2a 的 JSON-RPC 接口"""

import json
import sys
import uuid

import requests

BASE_URL = "http://localhost:3001"


def send_jsonrpc(method: str, params: dict, req_id=None):
    """发送 JSON-RPC 请求到 A2A 服务"""
    payload = {
        "jsonrpc": "2.0",
        "id": req_id or str(uuid.uuid4()),
        "method": method,
        "params": params,
    }
    print(f"\n>>> 请求: POST {BASE_URL}/")
    print(json.dumps(payload, ensure_ascii=False, indent=2))

    resp = requests.post(
        f"{BASE_URL}/",
        headers={"Content-Type": "application/json"},
        json=payload,
        timeout=300,
    )
    print(f"\n<<< 状态: {resp.status_code}")
    try:
        data = resp.json()
        print(json.dumps(data, ensure_ascii=False, indent=2))
        return data
    except Exception as e:
        print(f"解析失败: {e}")
        print(f"原始响应:\n{resp.text}")
        return None


def test_send_message():
    """测试 SendMessage（同步）"""
    print("=" * 60)
    print("测试 SendMessage")
    print("=" * 60)

    result = send_jsonrpc(
        method="SendMessage",
        params={
            "message": {
                "role": "ROLE_USER",
                "parts": [{"text": "列出工具列表"}],
            }
        },
    )

    if result and "result" in result:
        task = result["result"]
        print(f"\n✅ 任务 ID: {task.get('id')}")
        print(f"✅ 状态: {task.get('status', {}).get('state')}")
        if task.get("history"):
            for msg in task["history"]:
                role = msg.get("role", "unknown")
                text_parts = [p.get("text", "") for p in msg.get("parts", [])]
                print(f"  [{role}] {' '.join(text_parts)}")
    elif result and "error" in result:
        print(f"\n❌ JSON-RPC 错误: {result['error']}")
    return result


def test_send_streaming_message():
    """测试 SendStreamingMessage（SSE 流式）"""
    print("\n" + "=" * 60)
    print("测试 SendStreamingMessage (SSE)")
    print("=" * 60)

    payload = {
        "jsonrpc": "2.0",
        "id": str(uuid.uuid4()),
        "method": "SendMessage",
        "params": {
            "message": {
      
                "role": "ROLE_USER",
                "parts": [{"text": "取消周六上午9点半的会议室"}],
            }
        },
    }

    print(f">>> 请求: POST {BASE_URL}/")
    print(json.dumps(payload, ensure_ascii=False, indent=2))

    resp = requests.post(
        f"{BASE_URL}/",
        headers={
            "Content-Type": "application/json",
            "Accept": "text/event-stream",
        },
        json=payload,
        stream=True,
        timeout=300,
    )
    print(f"\n<<< 状态: {resp.status_code}")
    print(f"<<< Content-Type: {resp.headers.get('Content-Type')}")
    print("<<< 流式数据:\n")

    for line in resp.iter_lines(decode_unicode=True):
        if line.startswith("data:"):
            data_str = line[5:].strip()
            try:
                data = json.loads(data_str)
                print(json.dumps(data, ensure_ascii=False, indent=2))
                # 检测终端状态
                status = data.get("statusUpdate", {}).get("state") or data.get("task", {}).get("status", {}).get("state")
                if status in ("completed", "failed", "canceled"):
                    print(f"\n--- 流结束 (state={status}) ---")
            except json.JSONDecodeError:
                print(f"  [raw] {data_str}")


def test_rest_send_message():
    """测试 REST /message:send"""
    print("\n" + "=" * 60)
    print("测试 REST POST /message:send")
    print("=" * 60)

    payload = {
        "message": {
            "messageId": str(uuid.uuid4()),
            "role": "ROLE_USER",
            "parts": [{"text": "预定周六下午的会议室"}],
        }
    }

    print(f">>> 请求: POST {BASE_URL}/message:send")
    print(json.dumps(payload, ensure_ascii=False, indent=2))

    resp = requests.post(
        f"{BASE_URL}/message:send",
        headers={"Content-Type": "application/json"},
        json=payload,
        timeout=300,
    )
    print(f"\n<<< 状态: {resp.status_code}")
    try:
        data = resp.json()
        print(json.dumps(data, ensure_ascii=False, indent=2))
        return data
    except Exception as e:
        print(f"解析失败: {e}")
        print(f"原始响应:\n{resp.text}")
        return None


def test_get_agent_card():
    """测试获取 Agent Card"""
    print("\n" + "=" * 60)
    print("测试 GET /.well-known/agent-card.json")
    print("=" * 60)

    resp = requests.get(f"{BASE_URL}/.well-known/agent-card.json", timeout=10)
    print(f"<<< 状态: {resp.status_code}")
    try:
        data = resp.json()
        print(json.dumps(data, ensure_ascii=False, indent=2))
    except Exception:
        print(resp.text)


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description="测试 rust-agent-a2a")
    parser.add_argument(
        "--mode",
        choices=["jsonrpc", "stream", "rest", "card", "all"],
        default="all",
        help="测试模式",
    )
    args = parser.parse_args()

    if args.mode in ("card", "all"):
        test_get_agent_card()

    if args.mode in ("jsonrpc", "all"):
        test_send_message()

    if args.mode in ("stream", "all"):
        test_send_streaming_message()

    if args.mode in ("rest", "all"):
        test_rest_send_message()
