#!/usr/bin/env bash
# A2A 服务端快捷测试脚本（curl 版）
#
# 用法:
#   ./test_a2a.sh [BASE_URL]
# 默认 BASE_URL: http://localhost:3001

set -euo pipefail

BASE_URL="${1:-http://localhost:3001}"
PASS=0
FAIL=0

function ok() { echo "  ✅ $1"; ((PASS++)); }
function err() { echo "  ❌ $1"; ((FAIL++)); }

echo "A2A 服务端接口测试"
echo "目标地址: $BASE_URL"
echo "=================================================="

# 1. Agent Card
echo -e "\n[测试 1] GET /.well-known/agent.json"
RESP=$(curl -s -w "\n%{http_code}" "$BASE_URL/.well-known/agent.json" 2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
BODY=$(echo "$RESP" | sed '$d')
if [ "$HTTP_CODE" = "200" ]; then
    NAME=$(echo "$BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('name','unknown'))" 2>/dev/null || echo "unknown")
    SKILLS_COUNT=$(echo "$BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('skills',[])))" 2>/dev/null || echo "0")
    ok "Agent Card 有效，Agent: $NAME, Skills: $SKILLS_COUNT"
else
    err "状态码 $HTTP_CODE，响应: $BODY"
fi

# 2. 同步任务（跳过实际 LLM 调用，因为可能很慢）
echo -e "\n[测试 2] POST /tasks/send"
RESP=$(curl -s -w "\n%{http_code}" -X POST "$BASE_URL/tasks/send" \
    -H "Content-Type: application/json" \
    -d '{"id":"test-sync-001","message":{"role":"user","parts":[{"type":"text","text":"echo hello"}]}}' \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
if [ "$HTTP_CODE" = "200" ] || [ "$HTTP_CODE" = "503" ]; then
    ok "同步任务响应正常 (HTTP $HTTP_CODE)"
else
    err "意外状态码 $HTTP_CODE"
fi

# 3. 重复任务冲突
echo -e "\n[测试 3] 重复任务 ID 冲突检测"
RESP=$(curl -s -w "\n%{http_code}" -X POST "$BASE_URL/tasks/send" \
    -H "Content-Type: application/json" \
    -d '{"id":"test-dup-001","message":{"role":"user","parts":[{"type":"text","text":"hello"}]}}' \
    2>/dev/null || echo -e "\n000")
HTTP_CODE=$(echo "$RESP" | tail -1)
RESP2=$(curl -s -w "\n%{http_code}" -X POST "$BASE_URL/tasks/send" \
    -H "Content-Type: application/json" \
    -d '{"id":"test-dup-001","message":{"role":"user","parts":[{"type":"text","text":"hello"}]}}' \
    2>/dev/null || echo -e "\n000")
HTTP_CODE2=$(echo "$RESP2" | tail -1)
if [ "$HTTP_CODE2" = "409" ]; then
    ok "重复 ID 正确返回 409"
else
    err "期望 409，实际 $HTTP_CODE2"
fi

# 4. 查询不存在任务
echo -e "\n[测试 4] GET /tasks/{taskId} 404"
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/tasks/nonexistent-999" 2>/dev/null || echo "000")
if [ "$HTTP_CODE" = "404" ]; then
    ok "正确返回 404"
else
    err "期望 404，实际 $HTTP_CODE"
fi

# 5. 取消不存在任务
echo -e "\n[测试 5] POST /tasks/{taskId}/cancel 404"
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$BASE_URL/tasks/nonexistent-999/cancel" 2>/dev/null || echo "000")
if [ "$HTTP_CODE" = "404" ]; then
    ok "正确返回 404"
else
    err "期望 404，实际 $HTTP_CODE"
fi

# 6. SSE 流式（只验证连接）
echo -e "\n[测试 6] POST /tasks/sendSubscribe（SSE 流式连接）"
HTTP_CODE=$(curl -s -N -o /dev/null -w "%{http_code}" \
    -X POST "$BASE_URL/tasks/sendSubscribe" \
    -H "Content-Type: application/json" \
    -H "Accept: text/event-stream" \
    -d '{"id":"test-stream-001","message":{"role":"user","parts":[{"type":"text","text":"echo test"}]}}' \
    --max-time 5 \
    2>/dev/null || echo "000")
if [ "$HTTP_CODE" = "200" ] || [ "$HTTP_CODE" = "503" ]; then
    ok "SSE 流式连接正常 (HTTP $HTTP_CODE)"
else
    err "意外状态码 $HTTP_CODE"
fi

# 汇总
echo -e "\n=================================================="
echo "测试结果汇总: $PASS 通过, $FAIL 失败"
if [ "$FAIL" -eq 0 ]; then
    echo "🎉 全部通过!"
    exit 0
else
    exit 1
fi
