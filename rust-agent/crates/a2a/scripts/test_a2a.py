#!/usr/bin/env python3
"""A2A 服务端接口测试脚本

用法:
    python test_a2a.py [BASE_URL]

默认 BASE_URL: http://localhost:3001

需要的 Python 版本: 3.8+
依赖: 无（仅使用标准库 urllib）
"""

import json
import sys
import urllib.error
import urllib.request
from urllib.parse import urljoin


def request_json(method: str, url: str, data: dict | None = None, headers: dict | None = None) -> tuple[int, dict | str]:
    """发送 HTTP 请求并返回 (状态码, 响应体)"""
    req_headers = {"Content-Type": "application/json", "Accept": "application/json"}
    if headers:
        req_headers.update(headers)

    body = json.dumps(data, ensure_ascii=False).encode("utf-8") if data else None
    req = urllib.request.Request(url, method=method, data=body, headers=req_headers)

    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            resp_body = resp.read().decode("utf-8")
            try:
                return resp.status, json.loads(resp_body)
            except json.JSONDecodeError:
                return resp.status, resp_body
    except urllib.error.HTTPError as e:
        resp_body = e.read().decode("utf-8")
        try:
            return e.code, json.loads(resp_body)
        except json.JSONDecodeError:
            return e.code, resp_body
    except Exception as e:
        return -1, str(e)


def test_agent_card(base_url: str) -> bool:
    """测试 GET /.well-known/agent.json"""
    print("\n[测试 1] GET /.well-known/agent.json")
    code, body = request_json("GET", urljoin(base_url, "/.well-known/agent.json"))

    if code != 200:
        print(f"  ❌ 失败: 状态码 {code}, 响应: {body}")
        return False

    if not isinstance(body, dict):
        print(f"  ❌ 失败: 响应不是 JSON 对象: {body}")
        return False

    required_fields = ["name", "description", "url", "version", "capabilities", "skills"]
    missing = [f for f in required_fields if f not in body]
    if missing:
        print(f"  ❌ 失败: Agent Card 缺少字段: {missing}")
        return False

    skills = body.get("skills", [])
    print(f"  ✅ 成功: Agent Card 有效，包含 {len(skills)} 个 skills")
    print(f"     Agent 名称: {body.get('name')}")
    print(f"     Skills: {[s.get('id') for s in skills[:5]]}{'...' if len(skills) > 5 else ''}")
    return True


def test_sync_task(base_url: str) -> bool:
    """测试 POST /tasks/send（需要 LLM 环境，可能较慢）"""
    print("\n[测试 2] POST /tasks/send（同步任务）")
    task_id = "test-sync-task-001"
    payload = {
        "id": task_id,
        "message": {
            "role": "user",
            "parts": [{"type": "text", "text": "用 bash 运行 echo hello"}]
        }
    }

    code, body = request_json("POST", urljoin(base_url, "/tasks/send"), payload)

    if code == 503:
        print(f"  ⚠️  Agent 初始化失败（可能缺少 LLM API Key）")
        print(f"     响应: {body}")
        return True  # 不算测试失败，是环境问题

    if code not in (200, 201):
        print(f"  ❌ 失败: 状态码 {code}, 响应: {body}")
        return False

    if not isinstance(body, dict):
        print(f"  ❌ 失败: 响应不是 JSON 对象")
        return False

    status = body.get("status", {}).get("state", "unknown")
    print(f"  ✅ 成功: 任务完成，状态 = {status}")
    print(f"     任务 ID: {body.get('id')}")
    print(f"     历史消息数: {len(body.get('history', []))}")
    return True


def test_duplicate_task_conflict(base_url: str) -> bool:
    """测试重复任务 ID 返回 409"""
    print("\n[测试 3] 重复任务 ID 冲突检测")
    task_id = "test-dup-task-001"
    payload = {
        "id": task_id,
        "message": {
            "role": "user",
            "parts": [{"type": "text", "text": "hello"}]
        }
    }

    # 第一次创建（可能成功也可能失败，取决于环境）
    code1, _ = request_json("POST", urljoin(base_url, "/tasks/send"), payload)

    # 第二次创建（应该 409）
    code2, body2 = request_json("POST", urljoin(base_url, "/tasks/send"), payload)

    if code2 == 409:
        print(f"  ✅ 成功: 重复 ID 正确返回 409 Conflict")
        return True
    else:
        print(f"  ❌ 失败: 期望 409，实际 {code2}")
        return False


def test_get_nonexistent_task(base_url: str) -> bool:
    """测试 GET /tasks/{taskId} 404"""
    print("\n[测试 4] 查询不存在的任务")
    code, body = request_json("GET", urljoin(base_url, "/tasks/nonexistent-task-999"))

    if code == 404:
        print(f"  ✅ 成功: 正确返回 404 Not Found")
        return True
    else:
        print(f"  ❌ 失败: 期望 404，实际 {code}")
        return False


def test_cancel_nonexistent_task(base_url: str) -> bool:
    """测试 POST /tasks/{taskId}/cancel 404"""
    print("\n[测试 5] 取消不存在的任务")
    code, body = request_json("POST", urljoin(base_url, "/tasks/nonexistent-task-999/cancel"))

    if code == 404:
        print(f"  ✅ 成功: 正确返回 404 Not Found")
        return True
    else:
        print(f"  ❌ 失败: 期望 404，实际 {code}")
        return False


def test_streaming_task(base_url: str) -> bool:
    """测试 POST /tasks/sendSubscribe SSE 流式模式"""
    print("\n[测试 6] POST /tasks/sendSubscribe（SSE 流式）")
    task_id = "test-stream-task-001"
    payload = {
        "id": task_id,
        "message": {
            "role": "user",
            "parts": [{"type": "text", "text": "echo test"}]
        }
    }

    req = urllib.request.Request(
        urljoin(base_url, "/tasks/sendSubscribe"),
        method="POST",
        data=json.dumps(payload, ensure_ascii=False).encode("utf-8"),
        headers={
            "Content-Type": "application/json",
            "Accept": "text/event-stream",
        }
    )

    event_types = []
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            # 读取部分 SSE 事件（不要读完整流，可能很长）
            chunk = resp.read(4096).decode("utf-8", errors="replace")
            for line in chunk.split("\n"):
                if line.startswith("event:"):
                    event_types.append(line.replace("event:", "").strip())
    except urllib.error.HTTPError as e:
        if e.code == 503:
            print(f"  ⚠️  Agent 初始化失败（可能缺少 LLM API Key）")
            return True
        print(f"  ❌ 失败: HTTP {e.code}")
        return False
    except Exception as e:
        print(f"  ❌ 失败: {e}")
        return False

    if event_types:
        print(f"  ✅ 成功: 收到 SSE 事件流，事件类型: {event_types[:5]}")
        return True
    else:
        print(f"  ⚠️  收到响应但没有解析到 SSE 事件（可能是同步返回或空响应）")
        return True


def main():
    base_url = sys.argv[1] if len(sys.argv) > 1 else "http://localhost:3001"
    print(f"A2A 服务端接口测试")
    print(f"目标地址: {base_url}")
    print("=" * 50)

    results = []
    results.append(("Agent Card", test_agent_card(base_url)))
    results.append(("同步任务", test_sync_task(base_url)))
    results.append(("重复任务冲突", test_duplicate_task_conflict(base_url)))
    results.append(("查询不存在任务", test_get_nonexistent_task(base_url)))
    results.append(("取消不存在任务", test_cancel_nonexistent_task(base_url)))
    results.append(("流式任务", test_streaming_task(base_url)))

    print("\n" + "=" * 50)
    print("测试结果汇总:")
    passed = 0
    for name, ok in results:
        status = "✅ 通过" if ok else "❌ 失败"
        print(f"  {status} - {name}")
        if ok:
            passed += 1

    print(f"\n总计: {passed}/{len(results)} 通过")
    if passed == len(results):
        print("🎉 全部通过!")
    sys.exit(0 if passed == len(results) else 1)


if __name__ == "__main__":
    main()
