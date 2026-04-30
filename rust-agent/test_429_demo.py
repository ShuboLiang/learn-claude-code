#!/usr/bin/env python3
"""测试 429 限流错误的 SSE 原始响应"""

import json
import requests

BASE_URL = "http://localhost:3000"

# 1. 创建会话
resp = requests.post(f"{BASE_URL}/sessions")
sess = resp.json()
session_id = sess["id"]
print(f"Session ID: {session_id}")

# 2. 发送 [mock:429] 消息，流式接收原始 SSE
resp = requests.post(
    f"{BASE_URL}/sessions/{session_id}/messages",
    json={"content": "[mock:429] 测试限流"},
    stream=True,
    headers={"Accept": "text/event-stream"},
)

print("\n=== 原始 SSE 响应 ===")
for line in resp.iter_lines(decode_unicode=True):
    if line:
        print(line)

resp.close()
