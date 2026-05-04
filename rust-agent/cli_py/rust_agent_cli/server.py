"""Rust server 启动与生命周期管理."""

import os
import socket
import subprocess
import sys
from pathlib import Path


def _project_root() -> Path:
    if env_root := os.environ.get("RUST_AGENT_ROOT"):
        return Path(env_root)
    return Path(__file__).resolve().parents[2]


def find_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def resolve_server_binary() -> str:
    root = _project_root()
    if sys.platform == "win32":
        candidates = [
            root / "target" / "release" / "rust-agent-server.exe",
            root / "target" / "debug" / "rust-agent-server.exe",
        ]
    else:
        candidates = [
            root / "target" / "release" / "rust-agent-server",
            root / "target" / "debug" / "rust-agent-server",
        ]
    for p in candidates:
        if p.exists():
            return str(p)
    return str(candidates[0])


def start_server():
    """查找空闲端口，启动 Rust server，等待就绪.

    Returns (port, process).
    """
    binary = resolve_server_binary()
    port = find_free_port()
    process = subprocess.Popen(
        [binary, "--port", str(port)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    import urllib.request

    for _ in range(100):
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{port}/", timeout=1) as resp:
                if resp.status == 200:
                    return port, process
        except Exception:
            pass
        import time

        time.sleep(0.1)

    process.terminate()
    raise RuntimeError("server 启动超时")
