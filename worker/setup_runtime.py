"""Explicit one-time LiteRT installation; never called at normal app startup.

C++ inference executables are bundled with the Linux package. This installs only
the pinned CPU Python dependencies into a private venv. No models or user data
are read or uploaded. --wheelhouse enables fully offline runtime provisioning.
"""
import argparse
import ctypes
import json
import os
from pathlib import Path
import signal
import subprocess
import sys


def emit(status, **kwargs):
    print(json.dumps({"model":"runtime", "status":status, **kwargs}), flush=True)


def parent_death():
    ctypes.CDLL(None).prctl(1, signal.SIGKILL)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--install", action="store_true", required=True)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--wheelhouse", type=Path)
    args = parser.parse_args()
    os.umask(0o077)
    args.directory.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    env = dict(os.environ, PIP_DISABLE_PIP_VERSION_CHECK="1", PIP_NO_INPUT="1",
               PIP_CACHE_DIR=str(args.directory.parent / "wheel-cache"))

    # Windows packages bundle LiteRT, so this runs there only from a checkout.
    options = {"creationflags": subprocess.CREATE_NO_WINDOW} if sys.platform == "win32" else {"preexec_fn": parent_death}

    def run(argv):
        result = subprocess.run(argv, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                timeout=1200, **options)
        if result.returncode:
            raise RuntimeError("Runtime setup failed. Check available disk space and the internet connection, or retry using an offline wheels folder. The Linux package requires python3-venv.")

    emit("creating_environment")
    # Setup runs only while LiteRT is not ready. A cancelled or quit install can
    # leave a half-written package that pip would count as installed, so every
    # install starts from an empty venv (wheels stay in the cache). A checkout
    # on Windows may be running this from that venv: never clear it under us.
    clear = Path(sys.prefix).resolve() != args.directory.resolve()
    run([sys.executable, "-m", "venv", *(["--clear"] if clear else []), str(args.directory)])
    python = str(args.directory / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python"))
    lock = Path(__file__).resolve().with_name("requirements.lock.txt")
    emit("installing_pinned_dependencies")
    command = [python, "-m", "pip", "install", "--no-deps", "--no-input", "--disable-pip-version-check", "-r", str(lock)]
    command += ["--no-index", "--find-links", str(args.wheelhouse.resolve())] if args.wheelhouse else ["--index-url", "https://pypi.org/simple"]
    run(command)
    emit("checking_runtime")
    run([python, "-m", "pip", "check"])
    run([python, "-I", "-c", "from litert_lm import Backend, EmbeddingEngine, Content; assert callable(EmbeddingEngine.compute_embedding)"])
    emit("verified")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
        emit("error", message=str(error) if isinstance(error, RuntimeError) else "Runtime setup could not complete. Retry or choose an offline wheels folder.")
        sys.exit(1)
