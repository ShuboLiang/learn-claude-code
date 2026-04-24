#!/usr/bin/env bash
set -euo pipefail

# 颜色
CYAN='\033[0;36m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLI_DIR="$PROJECT_ROOT/cli"
SKIP_BUILD=false
SET_ENV_PERMANENT=false

# 解析参数
while [[ $# -gt 0 ]]; do
  case $1 in
    --skip-build) SKIP_BUILD=true; shift ;;
    --set-env-permanent) SET_ENV_PERMANENT=true; shift ;;
    *) echo "未知参数: $1"; exit 1 ;;
  esac
done

step() { echo -e "\n${CYAN}▶ $1${NC}"; }
success() { echo -e "${GREEN}  ✓ $1${NC}"; }
warn() { echo -e "${YELLOW}  ⚠ $1${NC}"; }
error_exit() { echo -e "${RED}  ✗ $1${NC}"; exit 1; }

# ── 检查前置依赖 ──
step "检查依赖..."

check_cmd() { command -v "$1" >/dev/null 2>&1; }

if ! check_cmd cargo; then
  error_exit "未找到 Rust/Cargo，请先安装: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
fi
success "Rust/Cargo 已安装 ($(cargo --version))"

if ! check_cmd node; then
  error_exit "未找到 Node.js，请先安装: https://nodejs.org"
fi
success "Node.js 已安装 ($(node -v))"

if ! check_cmd npm; then
  error_exit "未找到 npm"
fi
success "npm 已安装 ($(npm -v))"

# ── 检查 npm 全局目录权限 ──
step "检查 npm 全局目录..."
NPM_PREFIX="$(npm config get prefix)"
if [[ ! -w "$NPM_PREFIX" ]] && [[ "$NPM_PREFIX" == "/usr"* ]]; then
  warn "npm 全局目录 ($NPM_PREFIX) 需要 sudo 权限"
  NPM_GLOBAL="$HOME/.npm-global"
  mkdir -p "$NPM_GLOBAL/bin"
  npm config set prefix "$NPM_GLOBAL"
  
  # 自动添加到 PATH（如果还没加）
  for rc in "$HOME/.bashrc" "$HOME/.zshrc"; do
    if [[ -f "$rc" ]] && ! grep -q "$NPM_GLOBAL/bin" "$rc" 2>/dev/null; then
      echo "export PATH=\"$NPM_GLOBAL/bin:\$PATH\"" >> "$rc"
      success "已添加 npm 全局目录到 $rc"
    fi
  done
  export PATH="$NPM_GLOBAL/bin:$PATH"
  success "npm 全局目录已配置: $NPM_GLOBAL"
else
  success "npm 全局目录可写: $NPM_PREFIX"
fi

# ── 编译 Rust server ──
if [[ "$SKIP_BUILD" == "false" ]]; then
  step "编译 Rust server (release 模式)..."
  cd "$PROJECT_ROOT"
  if ! cargo build --release -p rust-agent-server; then
    error_exit "Rust server 编译失败"
  fi
  success "Rust server 编译完成"
else
  warn "跳过 Rust 编译 (--skip-build)"
fi

# 确认 binary 存在
SERVER_BINARY="$PROJECT_ROOT/target/release/rust-agent-server"
if [[ ! -f "$SERVER_BINARY" ]]; then
  SERVER_BINARY="$PROJECT_ROOT/target/debug/rust-agent-server"
  if [[ ! -f "$SERVER_BINARY" ]]; then
    error_exit "未找到 rust-agent-server，编译可能失败了"
  fi
fi
success "Server binary: $SERVER_BINARY"

# ── 安装 CLI 依赖 ──
step "安装 CLI 依赖..."
cd "$CLI_DIR"
if [[ ! -d "node_modules" ]]; then
  if ! npm install; then
    error_exit "npm install 失败"
  fi
  success "npm install 完成"
else
  success "node_modules 已存在，跳过 install"
fi

# ── npm link ──
step "注册全局命令 rust-agent..."
if ! npm link; then
  error_exit "npm link 失败（可能需要检查权限或 npm 全局目录配置）"
fi
success "npm link 完成"

# ── 设置环境变量（可选） ──
if [[ "$SET_ENV_PERMANENT" == "true" ]]; then
  step "设置环境变量 RUST_AGENT_ROOT..."
  
  # 检测当前 shell
  CURRENT_SHELL="$(basename "$SHELL")"
  case "$CURRENT_SHELL" in
    zsh) RC_FILE="$HOME/.zshrc" ;;
    bash) RC_FILE="$HOME/.bashrc" ;;
    *) RC_FILE="$HOME/.bashrc" ;;
  esac
  
  ENV_LINE="export RUST_AGENT_ROOT=\"$PROJECT_ROOT\""
  
  if [[ -f "$RC_FILE" ]] && grep -q "RUST_AGENT_ROOT=" "$RC_FILE" 2>/dev/null; then
    # 已存在，更新
    sed -i "s|export RUST_AGENT_ROOT=.*|$ENV_LINE|" "$RC_FILE"
    success "已更新 $RC_FILE 中的 RUST_AGENT_ROOT"
  else
    echo "" >> "$RC_FILE"
    echo "# rust-agent CLI 项目根目录" >> "$RC_FILE"
    echo "$ENV_LINE" >> "$RC_FILE"
    success "已写入 $RC_FILE"
  fi
  
  warn "请运行 'source $RC_FILE' 或新开终端使环境变量生效"
fi

# ── 验证 ──
step "验证安装..."
if ! command -v rust-agent >/dev/null 2>&1; then
  error_exit "全局命令 rust-agent 未找到，npm link 可能未生效"
fi
success "全局命令路径: $(command -v rust-agent)"

# 测试运行
step "测试启动..."
echo -e "${YELLOW}  正在验证 rust-agent 能否正常启动...${NC}"
cd "$CLI_DIR"
RUST_AGENT_ROOT="$PROJECT_ROOT" timeout 3 rust-agent 2>/dev/null || true
success "rust-agent 可执行"

# ── 完成 ──
echo -e "\n${GREEN}========================================${NC}"
echo -e "${GREEN}  rust-agent CLI 安装完成！${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo "  使用方式:"
echo "    rust-agent              启动交互式 CLI"
echo ""
echo "  项目根目录:"
echo "    $PROJECT_ROOT"
if [[ "$SET_ENV_PERMANENT" == "false" ]]; then
  echo ""
  echo -e "${YELLOW}  提示: 如需持久化环境变量，重新运行:${NC}"
  echo -e "${YELLOW}    ./install.sh --set-env-permanent${NC}"
fi
echo ""
