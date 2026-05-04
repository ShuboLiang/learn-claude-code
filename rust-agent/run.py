#!/usr/bin/env python3
"""一键启动器 — 支持 uv / pip 双模式，自动处理环境 + 编译 + 运行."""

import io
import os
import shutil
import subprocess
import sys
from pathlib import Path

# Windows 控制台默认 GBK，强制 UTF-8 输出
if sys.platform == "win32":
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", line_buffering=True)
    sys.stderr = io.TextIOWrapper(sys.stderr.buffer, encoding="utf-8", line_buffering=True)


def _project_root() -> Path:
    return Path(__file__).resolve().parent


def _which(name: str) -> str | None:
    return shutil.which(name)


def _run(cmd: list[str], cwd: Path | None = None, check: bool = True) -> None:
    print(f"▶ {' '.join(cmd)}")
    try:
        subprocess.run(cmd, cwd=cwd, check=check)
    except subprocess.CalledProcessError as e:
        print(f"✗ 命令失败 (exit={e.returncode})")
        sys.exit(1)


def _has_uv() -> bool:
    return _which("uv") is not None


def _ensure_uv() -> str:
    uv = _which("uv")
    if uv:
        return uv
    print("▶ 未找到 uv，尝试自动安装...")
    print("  访问 https://astral.sh/uv 手动安装，或按回车尝试自动安装")
    try:
        input()
    except KeyboardInterrupt:
        sys.exit(0)

    if sys.platform == "win32":
        # Windows: 用 PowerShell 安装 uv
        _run([
            "powershell", "-ExecutionPolicy", "Bypass",
            "-Command",
            "irm https://astral.sh/uv/install.ps1 | iex"
        ], check=False)
        # 刷新 PATH
        local_bin = Path.home() / ".local" / "bin"
        if local_bin.exists():
            os.environ["PATH"] = f"{local_bin}{os.pathsep}{os.environ['PATH']}"
    else:
        _run(["sh", "-c", "curl -LsSf https://astral.sh/uv/install.sh | sh"], check=False)
        local_bin = Path.home() / ".local" / "bin"
        if local_bin.exists():
            os.environ["PATH"] = f"{local_bin}{os.pathsep}{os.environ['PATH']}"

    uv = _which("uv")
    if not uv:
        print("✗ uv 安装后仍无法找到，请重启终端后重试")
        sys.exit(1)
    print("✓ uv 已安装")
    return uv


def _ensure_venv_uv(cli_py: Path) -> Path:
    """用 uv 创建虚拟环境，返回 python 可执行文件路径."""
    venv_dir = cli_py / ".venv"
    if sys.platform == "win32":
        py = venv_dir / "Scripts" / "python.exe"
    else:
        py = venv_dir / "bin" / "python"

    if not py.exists():
        print("▶ uv 创建虚拟环境...")
        _run(["uv", "venv", str(venv_dir)], cwd=cli_py)
        print("✓ 虚拟环境已创建")
    else:
        print("✓ 虚拟环境已存在")

    # uv sync / pip install
    print("▶ uv 安装依赖...")
    _run(["uv", "pip", "install", "-e", "."], cwd=cli_py)
    print("✓ 依赖就绪")
    return py


def _ensure_venv_pip(cli_py: Path) -> Path:
    """用标准 venv + pip 创建虚拟环境."""
    venv_dir = cli_py / ".venv"
    if sys.platform == "win32":
        py = venv_dir / "Scripts" / "python.exe"
        pip = venv_dir / "Scripts" / "pip.exe"
    else:
        py = venv_dir / "bin" / "python"
        pip = venv_dir / "bin" / "pip"

    if not py.exists():
        print("▶ 创建 Python 虚拟环境...")
        _run([sys.executable, "-m", "venv", str(venv_dir)])
        print("✓ 虚拟环境已创建")
    else:
        print("✓ 虚拟环境已存在")

    print("▶ pip 安装依赖...")
    _run([str(pip), "install", "-e", "."], cwd=cli_py)
    print("✓ 依赖就绪")
    return py


def _ensure_rust_server(root: Path, skip_build: bool) -> None:
    if sys.platform == "win32":
        release = root / "target" / "release" / "rust-agent-server.exe"
        debug = root / "target" / "debug" / "rust-agent-server.exe"
    else:
        release = root / "target" / "release" / "rust-agent-server"
        debug = root / "target" / "debug" / "rust-agent-server"

    if release.exists() or debug.exists():
        print("✓ Rust server 已存在")
        return

    if skip_build:
        print("✗ 未找到 Rust server，请去掉 --skip-build 重新运行")
        sys.exit(1)

    print("▶ 编译 Rust server (release)...")
    if not _which("cargo"):
        print("✗ 未找到 Cargo，请先安装 Rust: https://rustup.rs")
        sys.exit(1)
    _run(["cargo", "build", "--release", "-p", "rust-agent-server"], cwd=root)
    print("✓ Rust server 编译完成")


def main() -> None:
    skip_build = "--skip-build" in sys.argv or "-SkipBuild" in sys.argv

    root = _project_root()
    cli_py = root / "cli_py"

    print("========================================")
    print("  rust-agent 启动器")
    print("========================================")

    # ── 环境管理 ──
    if _has_uv():
        print("\n[使用 uv 管理环境]")
        _ensure_uv()
        py = _ensure_venv_uv(cli_py)
    else:
        print("\n[未检测到 uv，使用标准 venv + pip]")
        print("  提示: 安装 uv 可获得更快体验 → https://astral.sh/uv")
        py = _ensure_venv_pip(cli_py)

    # ── Rust server ──
    _ensure_rust_server(root, skip_build)

    # ── 启动 CLI ──
    print("\n========================================")
    print("  启动 rust-agent CLI")
    print("========================================\n")

    os.environ["RUST_AGENT_ROOT"] = str(root)
    os.execv(str(py), [str(py), "-m", "rust_agent_cli"])


if __name__ == "__main__":
    main()
