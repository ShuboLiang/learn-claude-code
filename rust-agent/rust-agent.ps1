#Requires -Version 5.1
<#
.SYNOPSIS
    一键启动 rust-agent Python CLI (uv 管理)

.DESCRIPTION
    1. 自动安装 uv（如未安装）
    2. uv 自动创建虚拟环境 + 安装依赖
    3. 自动编译 Rust server（如不存在）
    4. 启动 Textual CLI

.用法
    .\rust-agent.ps1              # 直接运行
    .\rust-agent.ps1 -SkipBuild   # 跳过 Rust 编译检查
#>

param([switch]$SkipBuild)

$ErrorActionPreference = "Stop"

# ── 定位项目根目录 ──
$ProjectRoot = $PSScriptRoot
if (-not $ProjectRoot) {
    $ProjectRoot = (Get-Location).Path
}

$CliPyDir = Join-Path $ProjectRoot "cli_py"

function Write-Step {
    param([string]$Message)
    Write-Host "`n▶ $Message" -ForegroundColor Cyan
}
function Write-Success {
    param([string]$Message)
    Write-Host "  ✓ $Message" -ForegroundColor Green
}

# ── 检查/安装 uv ──
Write-Step "检查 uv..."
$uv = Get-Command uv -ErrorAction SilentlyContinue
if (-not $uv) {
    Write-Step "自动安装 uv..."
    try {
        irm https://astral.sh/uv/install.ps1 | iex
        # 刷新 PATH（uv 默认安装到 ~/.local/bin 或 %USERPROFILE%\.local\bin）
        $localBin = Join-Path $env:USERPROFILE ".local\bin"
        if (Test-Path $localBin) {
            $env:Path = "$localBin;$env:Path"
        }
        $uv = Get-Command uv -ErrorAction SilentlyContinue
        if (-not $uv) {
            Write-Error "uv 安装后仍无法找到，请重启终端后重试"
        }
    } catch {
        Write-Error "uv 安装失败: $_"
    }
    Write-Success "uv 已安装"
} else {
    Write-Success "uv 已存在 ($($uv.Source))"
}

# ── 编译 Rust server ──
$ServerBinary = Join-Path $ProjectRoot "target\release\rust-agent-server.exe"
if (-not $SkipBuild) {
    if (-not (Test-Path $ServerBinary)) {
        Write-Step "编译 Rust server (release)..."
        $cargo = Get-Command cargo -ErrorAction SilentlyContinue
        if (-not $cargo) {
            Write-Error "未找到 Cargo，请先安装 Rust: https://rustup.rs"
        }
        Push-Location $ProjectRoot
        try {
            cargo build --release -p rust-agent-server
            if ($LASTEXITCODE -ne 0) { Write-Error "Rust 编译失败" }
        } finally {
            Pop-Location
        }
        Write-Success "Rust server 编译完成"
    } else {
        Write-Success "Rust server 已存在"
    }
} else {
    if (-not (Test-Path $ServerBinary)) {
        $ServerBinary = Join-Path $ProjectRoot "target\debug\rust-agent-server.exe"
    }
    if (-not (Test-Path $ServerBinary)) {
        Write-Error "未找到 rust-agent-server，请去掉 -SkipBuild 重新运行"
    }
}

# ── uv 自动管理依赖并运行 ──
Write-Step "启动 CLI (uv run)..."
Write-Host "  uv 会自动创建虚拟环境并安装依赖（首次稍慢）`n" -ForegroundColor DarkGray

$env:RUST_AGENT_ROOT = $ProjectRoot
Push-Location $CliPyDir
try {
    # uv run 会自动读取 pyproject.toml，创建 .venv，安装依赖，然后执行命令
    uv run python -m rust_agent_cli
} finally {
    Pop-Location
}
