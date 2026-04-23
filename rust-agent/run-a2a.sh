#!/usr/bin/env bash
#
# 启动 rust-agent-a2a 服务，支持预设 Agent 身份配置。
#
# 用法:
#   ./run-a2a.sh coding              # 启动代码审查 Agent（小明）
#   ./run-a2a.sh lark 8081           # 启动飞书 Agent，端口 8081
#   ./run-a2a.sh custom "小白" "文档生成" "./skills/doc" 3003
#
# 参数:
#   $1  preset     预设名称: coding / lark / general / custom
#   $2  nickname   昵称（custom 模式必填，其他模式可选覆盖）
#   $3  role       职位（custom 模式必填，其他模式可选覆盖）
#   $4  skills_dirs 技能目录，逗号分隔（可选覆盖）
#   $5  port        端口号（可选覆盖）

set -euo pipefail

PRESET="${1:-general}"
NICKNAME_OVERRIDE="${2:-}"
ROLE_OVERRIDE="${3:-}"
SKILLS_OVERRIDE="${4:-}"
PORT_OVERRIDE="${5:-0}"

# ── 预设配置 ──
case "$PRESET" in
  coding)
    NICKNAME="小明"
    ROLE="代码审查"
    SKILLS_DIRS="./skills/code-review,./skills/git"
    PORT="3001"
    ;;
  lark)
    NICKNAME="Lark 助手"
    ROLE="飞书办公"
    SKILLS_DIRS="./skills/lark-im,./skills/lark-doc,./skills/lark-calendar"
    PORT="3002"
    ;;
  general)
    NICKNAME=""
    ROLE=""
    SKILLS_DIRS=""
    PORT="3001"
    ;;
  custom)
    NICKNAME="${NICKNAME_OVERRIDE:-}"
    ROLE="${ROLE_OVERRIDE:-}"
    SKILLS_DIRS="${SKILLS_OVERRIDE:-}"
    PORT="3001"
    ;;
  *)
    echo "错误: 未知 preset '$PRESET'"
    echo "可用 preset: coding, lark, general, custom"
    exit 1
    ;;
esac

# 优先级：命令行参数 > preset
[ -n "$NICKNAME_OVERRIDE" ] && NICKNAME="$NICKNAME_OVERRIDE"
[ -n "$ROLE_OVERRIDE" ] && ROLE="$ROLE_OVERRIDE"
[ -n "$SKILLS_OVERRIDE" ] && SKILLS_DIRS="$SKILLS_OVERRIDE"
[ "$PORT_OVERRIDE" != "0" ] && PORT="$PORT_OVERRIDE"

export AGENT_NICKNAME="$NICKNAME"
export AGENT_ROLE="$ROLE"
export AGENT_SKILLS_DIRS="$SKILLS_DIRS"
export A2A_PORT="$PORT"

# ── 打印配置 ──
echo "========================================"
echo "  启动 A2A Agent: $PRESET"
echo "========================================"
echo "  昵称:     ${AGENT_NICKNAME:-(未设置)}"
echo "  职位:     ${AGENT_ROLE:-(未设置)}"
echo "  技能目录: ${AGENT_SKILLS_DIRS:-(默认)}"
echo "  端口:     $A2A_PORT"
echo "========================================"
echo ""

# ── 启动 ──
cargo run -p rust-agent-a2a
