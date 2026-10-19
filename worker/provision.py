"""The only network-capable application module. Never invoked at startup.

No third party Python packages required. Verified imports are atomic; --check is
strictly offline. Resume uses Range + Content-Range validation, never append a
server's full 200 response to a partial download.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "models" / "manifest.json"


def data_dir():
    if sys.platform == "win32":
        default = Path(os.environ.get("LOCALAPPDATA", Path.home() / "AppData/Local")) / "org.voxloom.desktop"
    else:
        default = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share")) / "voxloom"
    return Path(os.environ.get("VOXLOOM_DATA_DIR", default))


def emit(**payload):
    print(json.dumps(payload), flush=True)


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def valid(path, model):
    return path.is_file() and path.stat().st_size == model["bytes"] and digest(path) == model["sha256"]


def install(model, destination, import_path=None):
    destination.mkdir(parents=True, exist_ok=True, mode=0o700)
    target = destination / model["filename"]
    if valid(target, model):
        emit(model=model["id"], status="verified", received=model["bytes"], total=model["bytes"])
        return
    part = target.with_suffix(target.suffix + ".part")
    if import_path is not None:
        source = import_path / model["filename"] if import_path.is_dir() else import_path
        if not valid(source, model):
            raise ValueError(f"{model['name']}: import does not match pinned size and SHA-256")
        if source.resolve() != target.resolve():
            shutil.copyfile(source, part)
        else:
            return
    else:
        available = shutil.disk_usage(destination).free
        offset = part.stat().st_size if part.exists() else 0
        if available < model["bytes"] - offset + 64 * 1024 * 1024:
            raise ValueError("Not enough disk space (64 MiB reserve required)")
        url = f"https://huggingface.co/{model['repo']}/resolve/{model['revision']}/{model['filename']}"
        headers = {"User-Agent": "VoxLoom-model-setup/0.1"}
        if offset:
            headers["Range"] = f"bytes={offset}-"
        try:
            response = urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=60)
        except urllib.error.HTTPError as error:
            if error.code in (401, 403):
                raise ValueError(f"Access requires license acceptance. Visit {model['license_url']}, download through your browser, then use --offline-import. Credentials are never collected.") from None
            if error.code == 416 and valid(part, model):
                part.replace(target)
                return
            raise ValueError(f"Download returned HTTP {error.code}; retry or import from disk") from None
        with response:
            if response.status != 206:
                offset = 0
            elif not response.headers.get("Content-Range", "").startswith(f"bytes {offset}-"):
                raise ValueError("Invalid server resume offset")
            with part.open("ab" if offset else "wb") as output:
                os.chmod(part, 0o600)
                while block := response.read(1024 * 1024):
                    output.write(block)
                    offset += len(block)
                    if offset > model["bytes"]:
                        raise ValueError("Download exceeds pinned model size")
                    emit(model=model["id"], status="downloading", received=offset, total=model["bytes"])
                output.flush()
                os.fsync(output.fileno())
    if not valid(part, model):
        raise ValueError(f"{model['name']}: SHA-256 mismatch; remove the .part file and retry")
    os.chmod(part, 0o600)
    part.replace(target)
    emit(model=model["id"], status="verified", received=model["bytes"], total=model["bytes"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--check", action="store_true")
    action.add_argument("--download", action="store_true", help="explicit approval to download pinned weights")
    action.add_argument("--offline-import", type=Path)
    parser.add_argument("--core-only", action="store_true")
    parser.add_argument("--model", choices=["whisper", "whisper_accurate", "embedding", "summary_e2b", "summary_e4b"])
    parser.add_argument("--directory", type=Path, default=data_dir() / "models")
    args = parser.parse_args()
    models = json.loads(MANIFEST.read_text(encoding="utf-8"))["models"]
    missing = False
    for model in models:
        if model.get("manual_only") and args.model != model["id"]:
            continue
        if (args.core_only and model["optional"] and not args.model) or (args.model and args.model != model["id"]):
            continue
        if args.check:
            okay = valid(args.directory / model["filename"], model)
            emit(model=model["id"], status="verified" if okay else "missing_or_invalid", total=model["bytes"])
            missing |= not okay
        else:
            install(model, args.directory, args.offline_import)
    return int(missing)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, OSError, urllib.error.URLError) as error:
        emit(status="error", message=str(error))
        sys.exit(1)
