#!/usr/bin/env python3

from __future__ import annotations

import os
import subprocess


def run(argv: list[str]) -> None:
    proc = subprocess.run(argv)
    if proc.returncode != 0:
        raise SystemExit(proc.returncode)


def main() -> None:
    env = os.environ.copy()
    env.setdefault("CARGO_HOME", ".cargo_home")
    home = os.path.expanduser("~")
    root = os.path.join(home, ".local")
    run(
        [
            "env",
            *[f"{k}={v}" for k, v in env.items()],
            "cargo",
            "install",
            "--path",
            ".",
            "--root",
            root,
        ]
    )


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        raise SystemExit(130)
