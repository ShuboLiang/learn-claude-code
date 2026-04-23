#Requires -Version 7
<#
.SYNOPSIS
    启动 rust-agent-a2a 服务，支持预设 Agent 身份配置。

.DESCRIPTION
    通过预设或自定义环境变量启动 A2A 服务。
    预设配置包含昵称、职位、技能目录和端口。

.PARAMETER Agent
    选择预设的 Agent 配置：coding / lark / general / custom

.PARAMETER Port
    覆盖端口号（默认根据 preset 或 3001）

.PARAMETER Nickname
    自定义昵称（仅 custom 模式或覆盖 preset）

.PARAMETER Role
    自定义职位（仅 custom 模式或覆盖 preset）

.PARAMETER SkillsDirs
    自定义技能目录，逗号分隔（仅 custom 模式或覆盖 preset）

.EXAMPLE
    .\run-a2a.ps1 -Agent coding
    启动代码审查 Agent（小明）

.EXAMPLE
    .\run-a2a.ps1 -Agent custom -Nickname "小白" -Role "文档生成" -SkillsDirs ".\skills\doc"
    启动自定义 Agent

.EXAMPLE
    .\run-a2a.ps1 -Agent lark -Port 8081
    启动飞书 Agent，端口 8081
#>

param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("coding", "lark", "general", "custom")]
    [string]$Agent,

    [int]$Port = 0,

    [string]$Nickname = "",

    [string]$Role = "",

    [string]$SkillsDirs = ""
)

# ── 预设配置 ──
$Presets = @{
    coding = @{
        Nickname    = "梁舒勃"
        Role        = "代码审查"
        SkillsDirs  = "~/.rust-agent/skills"
        Port        = 3001
    }
    lark = @{
        Nickname    = " Lark 助手"
        Role        = "飞书办公"
        SkillsDirs  = ".\skills\lark-im,.\skills\lark-doc,.\skills\lark-calendar"
        Port        = 3002
    }
    general = @{
        Nickname    = ""
        Role        = ""
        SkillsDirs  = ""
        Port        = 3001
    }
    custom = @{
        Nickname    = ""
        Role        = ""
        SkillsDirs  = ""
        Port        = 3001
    }
}

$preset = $Presets[$Agent]

# 优先级：命令行参数 > preset > 空
$env:AGENT_NICKNAME = if ($Nickname) { $Nickname } else { $preset.Nickname }
$env:AGENT_ROLE     = if ($Role)     { $Role }     else { $preset.Role }
$env:AGENT_SKILLS_DIRS = if ($SkillsDirs) { $SkillsDirs } else { $preset.SkillsDirs }
$env:A2A_PORT       = if ($Port -gt 0) { $Port } else { $preset.Port }

# ── 打印配置 ──
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  启动 A2A Agent: $Agent" -ForegroundColor Cyan
Write-Host "========================================"
Write-Host "  昵称:     $(if ($env:AGENT_NICKNAME) { $env:AGENT_NICKNAME } else { "(未设置)" })"
Write-Host "  职位:     $(if ($env:AGENT_ROLE) { $env:AGENT_ROLE } else { "(未设置)" })"
Write-Host "  技能目录: $(if ($env:AGENT_SKILLS_DIRS) { $env:AGENT_SKILLS_DIRS } else { "(默认)" })"
Write-Host "  端口:     $($env:A2A_PORT)"
Write-Host "========================================"
Write-Host ""

# ── 启动 ──
cargo run -p rust-agent-a2a
