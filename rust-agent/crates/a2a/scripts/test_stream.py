#!/usr/bin/env python3
"""A2A 流式发送消息测试脚本

用法:
    python test_stream.py [BASE_URL]

默认 BASE_URL: http://localhost:3001
"""

import json
import sys
import urllib.request


def stream_message(base_url: str, text: str) -> None:
    """流式发送消息并通过 SSE 接收响应"""
    url = f"{base_url}/message:stream"
    payload = {
        "message": {
            "messageId": "msg-stream-001",
            "role": "ROLE_USER",
            "parts": [{"text": text}]
        }
    }

    req = urllib.request.Request(
        url,
        method="POST",
        data=json.dumps(payload, ensure_ascii=False).encode("utf-8"),
        headers={
            "Content-Type": "application/json",
            "Accept": "text/event-stream",
        }
    )

    print(f"发送流式请求到: {url}")
    print(f"消息内容: {text}")
    print("-" * 50)

    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            # 按行读取 SSE 流
            buffer = b""
            while True:
                chunk = resp.read(1)
                if not chunk:
                    break
                buffer += chunk
                if b"\n" in buffer:
                    lines = buffer.split(b"\n")
                    buffer = lines.pop()
                    for line in lines:
                        line_str = line.decode("utf-8", errors="replace").strip()
                        if line_str.startswith("data:"):
                            data = line_str[5:].strip()
                            try:
                                obj = json.loads(data)
                                # 提取并打印关键信息
                                if "message" in obj:
                                    msg = obj["message"]
                                    text_parts = [p.get("text", "") for p in msg.get("parts", []) if "text" in p]
                                    print(f"[消息] {''.join(text_parts)}")
                                elif "statusUpdate" in obj:
                                    state = obj["statusUpdate"].get("status", {}).get("state", "unknown")
                                    print(f"[状态] {state}")
                                elif "artifactUpdate" in obj:
                                    name = obj["artifactUpdate"].get("artifact", {}).get("name", "unknown")
                                    print(f"[产出] {name}")
                                elif "task" in obj:
                                    state = obj["task"].get("status", {}).get("state", "unknown")
                                    print(f"[任务] 状态 = {state}")
                                else:
                                    print(f"[原始] {data}")
                            except json.JSONDecodeError:
                                print(f"[数据] {data}")
    except urllib.error.HTTPError as e:
        print(f"请求失败: HTTP {e.code}")
        body = e.read().decode("utf-8", errors="replace")
        print(f"响应: {body}")
    except Exception as e:
        print(f"请求异常: {e}")


if __name__ == "__main__":
    base_url = sys.argv[1] if len(sys.argv) > 1 else "http://localhost:3001"
    stream_message(base_url, "你好")
