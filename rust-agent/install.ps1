#Requires -Version 7
<#
.SYNOPSIS
    一键部署 rust-agent CLI 为全局命令

.DESCRIPTION
    1. 编译 Rust server (release)
    2. 安装 CLI npm 依赖
    3. npm link 注册全局命令 rust-agent
    4. 可选：持久化 RUST_AGENT_ROOT 环境变量
#>

param(
    [switch]$SkipBuild,
    [switch]$SetEnvPermanent
)

$ErrorActionPreference = "Stop"
$ProjectRoot = $PSScriptRoot
$CliDir = Join-Path $ProjectRoot "cli"

function Test-CommandExists {
    param([string]$Name)
    return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

function Write-Step {
    param([string]$Message)
    Write-Host "`n▶ $Message" -ForegroundColor Cyan
}

function Write-Success {
    param([string]$Message)
    Write-Host "  ✓ $Message" -ForegroundColor Green
}

function Write-Warn {
    param([string]$Message)
    Write-Host "  ⚠ $Message" -ForegroundColor Yellow
}

function Write-ErrorExit {
    param([string]$Message)
    Write-Host "  ✗ $Message" -ForegroundColor Red
    exit 1
}

# ── 检查前置依赖 ──
Write-Step "检查依赖..."

if (-not (Test-CommandExists "cargo")) {
    Write-ErrorExit "未找到 Rust/Cargo，请先安装：https://rustup.rs"
}
Write-Success "Rust/Cargo 已安装"

if (-not (Test-CommandExists "node")) {
    Write-ErrorExit "未找到 Node.js，请先安装：https://nodejs.org"
}
Write-Success "Node.js 已安装 ($(node -v))"

if (-not (Test-CommandExists "npm")) {
    Write-ErrorExit "未找到 npm"
}
Write-Success "npm 已安装 ($(npm -v))"

# ── 编译 Rust server ──
if (-not $SkipBuild) {
    Write-Step "编译 Rust server (release 模式)..."
    Push-Location $ProjectRoot
    try {
        cargo build --release -p rust-agent-server 2>&1 | ForEach-Object {
            if ($_ -match "error") { Write-Host "    $_" -ForegroundColor Red }
            elseif ($_ -match "warning|Compiling|Finished") { Write-Host "    $_" -ForegroundColor DarkGray }
        }
        if ($LASTEXITCODE -ne 0) { Write-ErrorExit "Rust server 编译失败" }
    } finally {
        Pop-Location
    }
    Write-Success "Rust server 编译完成"
} else {
    Write-Warn "跳过 Rust 编译（--SkipBuild）"
}

# 确认 binary 存在
$ServerBinary = Join-Path $ProjectRoot "target/release/rust-agent-server.exe"
if (-not (Test-Path $ServerBinary)) {
    $ServerBinary = Join-Path $ProjectRoot "target/debug/rust-agent-server.exe"
    if (-not (Test-Path $ServerBinary)) {
        Write-ErrorExit "未找到 rust-agent-server.exe，编译可能失败了"
    }
}
Write-Success "Server binary: $ServerBinary"

# ── 安装 CLI 依赖 ──
Write-Step "安装 CLI 依赖..."
Push-Location $CliDir
try {
    if (-not (Test-Path (Join-Path $CliDir "node_modules"))) {
        npm install 2>&1 | ForEach-Object {
            if ($_ -match "ERR" -or $_ -match "error") { Write-Host "    $_" -ForegroundColor Red }
            elseif ($_ -match "added|packages") { Write-Host "    $_" -ForegroundColor DarkGray }
        }
        if ($LASTEXITCODE -ne 0) { Write-ErrorExit "npm install 失败" }
        Write-Success "npm install 完成"
    } else {
        Write-Success "node_modules 已存在，跳过 install"
    }
} finally {
    Pop-Location
}

# ── npm link ──
Write-Step "注册全局命令 rust-agent..."
Push-Location $CliDir
try {
    $linkOutput = npm link 2>&1
    if ($LASTEXITCODE -ne 0) {
        if ($linkOutput -match "EPERM|EACCES|administrator") {
            Write-ErrorExit "npm link 需要管理员权限，请用管理员 PowerShell 重新运行本脚本"
        }
        Write-ErrorExit "npm link 失败: $linkOutput"
    }
    Write-Success "npm link 完成"
} finally {
    Pop-Location
}

# ── 设置环境变量（可选） ──
if ($SetEnvPermanent) {
    Write-Step "设置环境变量 RUST_AGENT_ROOT..."
    $current = [Environment]::GetEnvironmentVariable("RUST_AGENT_ROOT", "User")
    if ($current -eq $ProjectRoot) {
        Write-Success "环境变量已设置，无需更改"
    } else {
        [Environment]::SetEnvironmentVariable("RUST_AGENT_ROOT", $ProjectRoot, "User")
        Write-Success "RUST_AGENT_ROOT = $ProjectRoot （已写入用户环境变量）"
        Write-Warn "新终端窗口才会生效"
    }
}

# ── 验证 ──
Write-Step "验证安装..."
$rustAgentCmd = Get-Command "rust-agent" -ErrorAction SilentlyContinue
if (-not $rustAgentCmd) {
    Write-ErrorExit "全局命令 rust-agent 未找到，npm link 可能未生效"
}
Write-Success "全局命令路径: $($rustAgentCmd.Source)"

# 测试运行一次（只验证能启动）
Write-Step "测试启动..."
Write-Host "  正在验证 rust-agent 能否正常启动（按 Ctrl+C 退出测试）...`n" -ForegroundColor DarkGray
Push-Location $CliDir
try {
    # 设置临时环境变量用于测试
    $env:RUST_AGENT_ROOT = $ProjectRoot
    rust-agent --help 2>$null
    if ($LASTEXITCODE -eq 0 -or $LASTEXITCODE -eq 1) {
        Write-Success "rust-agent 可执行"
    }
} catch {
    Write-Warn "无法自动验证启动，请手动运行 `rust-agent` 测试"
} finally {
    Pop-Location
}

# ── 完成 ──
Write-Host "`n========================================" -ForegroundColor Green
Write-Host "  rust-agent CLI 安装完成！" -ForegroundColor Green
Write-Host "========================================" -ForegroundColor Green
Write-Host ""
Write-Host "  使用方式:"
Write-Host "    rust-agent              启动交互式 CLI"
Write-Host ""
Write-Host "  环境变量:"
Write-Host "    RUST_AGENT_ROOT = $ProjectRoot"
if (-not $SetEnvPermanent) {
    Write-Host ""
    Write-Host "  提示: 如需持久化环境变量，重新运行:" -ForegroundColor Yellow
    Write-Host "    .\install.ps1 -SetEnvPermanent" -ForegroundColor Yellow
}
Write-Host ""
