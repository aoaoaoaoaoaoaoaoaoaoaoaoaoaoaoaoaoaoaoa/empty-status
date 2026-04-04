from __future__ import annotations

import json
import os
import re
import select
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
import uuid
from datetime import datetime, timezone
from pathlib import Path

CODEX_APP_SERVER_TIMEOUT_SECONDS = 6.0
CODEX_APP_SERVER_COMMAND = [
    "codex",
    "app-server",
    "--listen",
    "stdio://",
    "--session-source",
    "statusline",
]
CODEX_CLIENT_INFO = {
    "name": "empty-status-probe",
    "version": "0.0.0",
}
CLAUDE_REFRESH_SUCCESS_SECONDS = 15.0 * 60.0
CLAUDE_BOOT_TIMEOUT_SECONDS = 8.0
CLAUDE_VIEW_TIMEOUT_SECONDS = 8.0
CLAUDE_PANE_HEIGHT = 40
CLAUDE_PANE_WIDTH = 120
CLAUDE_STATUS_COMMAND = [
    "claude",
    "--model",
    "haiku",
    "--effort",
    "low",
    "--tools",
    "",
]
CLAUDE_SESSION_USAGE_RE = re.compile(
    r"Current session\s*\n.*?(\d+)% used\s*\n\s*Resets ([^\n]+)",
    re.DOTALL,
)
CLAUDE_WEEKLY_USAGE_RE = re.compile(
    r"Current week \(all models\)\s*\n.*?(\d+)% used\s*\n\s*Resets ([^\n]+)",
    re.DOTALL,
)


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def xdg_cache_home() -> Path:
    raw = os.environ.get("XDG_CACHE_HOME")
    if raw:
        return Path(raw)
    return Path.home() / ".cache"


def claude_cache_path() -> Path:
    return xdg_cache_home() / "empty-status" / "claude-rate-limits.json"


def clamp_percent(value: object) -> int | None:
    if not isinstance(value, (int, float)):
        return None
    return max(0, min(100, int(round(value))))


def parse_rfc3339_age_seconds(raw: str) -> float | None:
    try:
        parsed = datetime.fromisoformat(raw.replace("Z", "+00:00"))
    except ValueError:
        return None
    return max(0.0, (datetime.now(timezone.utc) - parsed).total_seconds())


def newest_project_files(path: Path) -> list[Path]:
    ranked: list[tuple[float, Path]] = []
    try:
        projects = [child for child in path.iterdir() if child.is_dir()]
    except FileNotFoundError:
        return []

    for project in projects:
        try:
            files = [child for child in project.iterdir() if child.is_file() and child.suffix == ".jsonl"]
        except OSError:
            continue
        for file in files:
            try:
                ranked.append((file.stat().st_mtime, file))
            except OSError:
                continue

    ranked.sort(key=lambda item: item[0], reverse=True)
    return [file for _, file in ranked]


def reverse_text_lines(path: Path, block_size: int = 1 << 16):
    with path.open("rb") as handle:
        handle.seek(0, os.SEEK_END)
        offset = handle.tell()
        remainder = b""
        while offset > 0:
            span = min(block_size, offset)
            offset -= span
            handle.seek(offset)
            chunk = handle.read(span)
            lines = (chunk + remainder).split(b"\n")
            remainder = lines[0]
            for raw in reversed(lines[1:]):
                yield raw.decode("utf-8", "replace")
        if remainder:
            yield remainder.decode("utf-8", "replace")


def read_json_rpc_line(handle, timeout_seconds: float) -> dict[str, object] | None:
    ready, _, _ = select.select([handle], [], [], timeout_seconds)
    if not ready:
        return None

    line = handle.readline()
    if not line:
        return None

    try:
        message = json.loads(line)
    except json.JSONDecodeError:
        return None
    return message if isinstance(message, dict) else None


def rpc_call(
    proc: subprocess.Popen[str],
    request_id: int,
    method: str,
    params: dict[str, object],
    timeout_seconds: float,
) -> dict[str, object] | None:
    if proc.stdin is None or proc.stdout is None:
        return None

    proc.stdin.write(
        json.dumps(
            {
                "jsonrpc": "2.0",
                "id": request_id,
                "method": method,
                "params": params,
            },
            separators=(",", ":"),
        )
        + "\n"
    )
    proc.stdin.flush()

    deadline = time.monotonic() + timeout_seconds
    while time.monotonic() < deadline:
        remaining = max(0.0, deadline - time.monotonic())
        message = read_json_rpc_line(proc.stdout, remaining)
        if message is None:
            return None
        if message.get("id") != request_id:
            continue
        result = message.get("result")
        return result if isinstance(result, dict) else None
    return None


def snapshot_field(snapshot: dict[str, object], *names: str) -> object:
    for name in names:
        value = snapshot.get(name)
        if value is not None:
            return value
    return None


def select_window(snapshot: dict[str, object], duration_minutes: int) -> dict[str, object] | None:
    for key in ("primary", "secondary"):
        window = snapshot.get(key)
        if (
            isinstance(window, dict)
            and snapshot_field(window, "window_minutes", "windowDurationMins") == duration_minutes
        ):
            return window
    return None


def read_codex_quota() -> dict[str, object] | None:
    if shutil.which("codex") is None:
        return None

    try:
        proc = subprocess.Popen(
            CODEX_APP_SERVER_COMMAND,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
        )
    except OSError:
        return None

    try:
        initialized = rpc_call(
            proc,
            1,
            "initialize",
            {
                "clientInfo": CODEX_CLIENT_INFO,
                "capabilities": {"experimentalApi": True},
            },
            CODEX_APP_SERVER_TIMEOUT_SECONDS,
        )
        if initialized is None:
            return None

        result = rpc_call(
            proc,
            2,
            "account/rateLimits/read",
            {},
            CODEX_APP_SERVER_TIMEOUT_SECONDS,
        )
        if result is None:
            return None

        snapshots = snapshot_field(result, "rateLimitsByLimitId")
        if isinstance(snapshots, dict):
            snapshot = snapshots.get("codex")
            if not isinstance(snapshot, dict):
                return None
        else:
            snapshot = snapshot_field(result, "rateLimits")
            if not isinstance(snapshot, dict):
                return None

        weekly = select_window(snapshot, 10080)
        if weekly is None:
            return None
        five_hour = select_window(snapshot, 300)
        weekly_used = clamp_percent(snapshot_field(weekly, "used_percent", "usedPercent"))
        if weekly_used is None:
            return None
        return {
            "captured_at": utc_now(),
            "weekly_used_percent": weekly_used,
            "weekly_resets_at": snapshot_field(weekly, "resets_at", "resetsAt"),
            "five_hour_used_percent": clamp_percent(
                None
                if five_hour is None
                else snapshot_field(five_hour, "used_percent", "usedPercent")
            ),
            "five_hour_resets_at": None
            if five_hour is None
            else snapshot_field(five_hour, "resets_at", "resetsAt"),
            "plan_type": snapshot_field(snapshot, "plan_type", "planType"),
        }
    except (BrokenPipeError, OSError, subprocess.SubprocessError):
        return None
    finally:
        if proc.stdin is not None:
            proc.stdin.close()
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=1.0)
            except subprocess.TimeoutExpired:
                proc.kill()
                try:
                    proc.wait(timeout=1.0)
                except subprocess.TimeoutExpired:
                    pass


def load_claude_cache() -> dict[str, object] | None:
    target = claude_cache_path()
    try:
        parsed = json.loads(target.read_text(encoding="utf-8"))
    except (FileNotFoundError, OSError, json.JSONDecodeError):
        return None
    return parsed if isinstance(parsed, dict) else None


def write_claude_cache(snapshot: dict[str, object]) -> None:
    target = claude_cache_path()
    target.parent.mkdir(parents=True, exist_ok=True)
    temp = target.with_suffix(".json.tmp")
    temp.write_text(json.dumps(snapshot, separators=(",", ":")), encoding="utf-8")
    temp.replace(target)


def read_recent_claude_cwd() -> Path | None:
    root = Path.home() / ".claude" / "projects"
    probed_files = 0
    for file in newest_project_files(root):
        probed_files += 1
        if probed_files > 96:
            return None
        try:
            for line in reverse_text_lines(file):
                if '"cwd"' not in line:
                    continue
                event = json.loads(line)
                cwd = event.get("cwd")
                if not isinstance(cwd, str):
                    continue
                path = Path(cwd)
                if path.is_dir():
                    return path
        except (OSError, UnicodeDecodeError, json.JSONDecodeError):
            continue
    return None


def run_tmux(base: list[str], cwd: Path, *args: str, capture: bool = False) -> str:
    result = subprocess.run(
        [*base, *args],
        check=True,
        cwd=cwd,
        env={**os.environ, "TMUX_TMPDIR": tempfile.gettempdir()},
        stdout=subprocess.PIPE if capture else subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        text=True,
    )
    return "" if result.stdout is None else result.stdout


def wait_for_tmux_pane(
    base: list[str],
    cwd: Path,
    predicate,
    timeout_seconds: float,
) -> str:
    import time

    deadline = time.monotonic() + timeout_seconds
    last = ""
    while time.monotonic() < deadline:
        last = run_tmux(base, cwd, "capture-pane", "-p", "-S", "-", capture=True)
        if predicate(last):
            return last
        time.sleep(0.25)
    return last


def parse_claude_usage_pane(pane: str) -> dict[str, object] | None:
    session_match = CLAUDE_SESSION_USAGE_RE.search(pane)
    weekly_match = CLAUDE_WEEKLY_USAGE_RE.search(pane)
    if session_match is None or weekly_match is None:
        return None

    five_hour_used = clamp_percent(int(session_match.group(1)))
    weekly_used = clamp_percent(int(weekly_match.group(1)))
    if five_hour_used is None or weekly_used is None:
        return None

    return {
        "captured_at": utc_now(),
        "five_hour_used_percent": five_hour_used,
        "five_hour_resets_at": session_match.group(2).strip(),
        "weekly_used_percent": weekly_used,
        "weekly_resets_at": weekly_match.group(2).strip(),
    }


def probe_claude_quota() -> dict[str, object] | None:
    if shutil.which("claude") is None or shutil.which("tmux") is None:
        return None

    cwd = read_recent_claude_cwd()
    if cwd is None:
        return None

    socket_dir = Path(tempfile.mkdtemp(prefix="empty-status-quota-"))
    socket_path = socket_dir / f"{uuid.uuid4().hex}.sock"
    base = ["tmux", "-S", str(socket_path)]

    try:
        run_tmux(
            base,
            cwd,
            "new-session",
            "-d",
            "-x",
            str(CLAUDE_PANE_WIDTH),
            "-y",
            str(CLAUDE_PANE_HEIGHT),
            shlex.join(CLAUDE_STATUS_COMMAND),
        )
        if "❯" not in wait_for_tmux_pane(
            base,
            cwd,
            lambda pane: "❯" in pane,
            CLAUDE_BOOT_TIMEOUT_SECONDS,
        ):
            return None

        run_tmux(base, cwd, "send-keys", "/status", "Enter")
        wait_for_tmux_pane(
            base,
            cwd,
            lambda pane: "Version:" in pane and "Status" in pane,
            CLAUDE_VIEW_TIMEOUT_SECONDS,
        )

        run_tmux(base, cwd, "send-keys", "Right")
        wait_for_tmux_pane(
            base,
            cwd,
            lambda pane: "Search settings..." in pane,
            CLAUDE_VIEW_TIMEOUT_SECONDS,
        )

        run_tmux(base, cwd, "send-keys", "Right")
        pane = wait_for_tmux_pane(
            base,
            cwd,
            lambda pane: "Current week (all models)" in pane,
            CLAUDE_VIEW_TIMEOUT_SECONDS,
        )
        snapshot = parse_claude_usage_pane(pane)
        if snapshot is not None:
            write_claude_cache(snapshot)
            return snapshot
        return load_claude_cache()
    except (OSError, subprocess.SubprocessError):
        return load_claude_cache()
    finally:
        subprocess.run(
            [*base, "kill-server"],
            cwd=cwd,
            env={**os.environ, "TMUX_TMPDIR": tempfile.gettempdir()},
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
            text=True,
        )
        shutil.rmtree(socket_dir, ignore_errors=True)


def read_claude_quota(force_refresh: bool) -> dict[str, object] | None:
    cached = load_claude_cache()
    if not force_refresh and cached is not None:
        captured_at = cached.get("captured_at")
        if isinstance(captured_at, str):
            age_seconds = parse_rfc3339_age_seconds(captured_at)
            if age_seconds is not None and age_seconds < CLAUDE_REFRESH_SUCCESS_SECONDS:
                return cached

    refreshed = probe_claude_quota()
    if refreshed is not None:
        return refreshed
    return cached


def main() -> None:
    force_refresh = "--force" in sys.argv[1:]
    snapshot = {
        "sampled_at": utc_now(),
        "codex": read_codex_quota(),
        "claude": read_claude_quota(force_refresh),
    }
    print(json.dumps(snapshot, separators=(",", ":")), flush=True)


if __name__ == "__main__":
    main()
