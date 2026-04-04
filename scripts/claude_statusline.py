#!/usr/bin/env python3

from __future__ import annotations

import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def xdg_cache_home() -> Path:
    raw = os.environ.get("XDG_CACHE_HOME")
    if raw:
        return Path(raw)
    return Path.home() / ".cache"


def cache_path() -> Path:
    return xdg_cache_home() / "empty-status" / "claude-rate-limits.json"


def clamp_percent(value: object) -> int | None:
    if not isinstance(value, (int, float)):
        return None
    return max(0, min(100, int(round(value))))


def load_payload() -> dict[str, object]:
    raw = sys.stdin.read()
    if not raw.strip():
        return {}
    try:
        parsed = json.loads(raw)
    except json.JSONDecodeError:
        return {}
    return parsed if isinstance(parsed, dict) else {}


def write_cache(snapshot: dict[str, object]) -> None:
    target = cache_path()
    target.parent.mkdir(parents=True, exist_ok=True)
    temp = target.with_suffix(".json.tmp")
    temp.write_text(json.dumps(snapshot, separators=(",", ":")), encoding="utf-8")
    temp.replace(target)


def load_cache() -> dict[str, object] | None:
    target = cache_path()
    try:
        parsed = json.loads(target.read_text(encoding="utf-8"))
    except (FileNotFoundError, OSError, json.JSONDecodeError):
        return None
    return parsed if isinstance(parsed, dict) else None


def maybe_extract_rate_limits(payload: dict[str, object]) -> dict[str, object] | None:
    rate_limits = payload.get("rate_limits")
    if not isinstance(rate_limits, dict):
        return None

    five_hour = rate_limits.get("five_hour")
    weekly = rate_limits.get("seven_day")
    if not isinstance(five_hour, dict) or not isinstance(weekly, dict):
        return None

    five_hour_used = clamp_percent(five_hour.get("used_percentage"))
    weekly_used = clamp_percent(weekly.get("used_percentage"))
    five_hour_reset = five_hour.get("resets_at")
    weekly_reset = weekly.get("resets_at")
    if None in (five_hour_used, weekly_used, five_hour_reset, weekly_reset):
        return None

    return {
        "captured_at": utc_now(),
        "five_hour_used_percent": five_hour_used,
        "five_hour_resets_at": five_hour_reset,
        "weekly_used_percent": weekly_used,
        "weekly_resets_at": weekly_reset,
    }


def render_summary(snapshot: dict[str, object] | None, payload: dict[str, object]) -> str:
    if snapshot is not None:
        five_hour_remaining = 100 - int(snapshot["five_hour_used_percent"])
        weekly_remaining = 100 - int(snapshot["weekly_used_percent"])
        return f"cc 7d {weekly_remaining:>3}% 5h {five_hour_remaining:>3}%"

    context = payload.get("context_window")
    if isinstance(context, dict):
        remaining = clamp_percent(context.get("remaining_percentage"))
        if remaining is not None:
            return f"ctx {remaining:>3}%"

    return ""


def main() -> None:
    payload = load_payload()
    snapshot = maybe_extract_rate_limits(payload)
    if snapshot is not None:
        write_cache(snapshot)
    else:
        snapshot = load_cache()

    print(render_summary(snapshot, payload), end="")


if __name__ == "__main__":
    main()
