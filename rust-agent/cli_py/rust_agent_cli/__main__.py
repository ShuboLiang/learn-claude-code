import sys

from rust_agent_cli.app import RustAgentApp
from rust_agent_cli.server import start_server


def main():
    port, process = start_server()
    app = RustAgentApp(server_port=port, server_process=process)
    try:
        app.run()
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except Exception:
            process.kill()


if __name__ == "__main__":
    main()
