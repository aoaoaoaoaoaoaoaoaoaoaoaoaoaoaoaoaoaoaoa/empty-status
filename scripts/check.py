#!/usr/bin/env python3

from __future__ import annotations

import os
import subprocess


def run(argv: list[str]) -> None:
    proc = subprocess.run(argv)
    if proc.returncode != 0:
        raise SystemExit(proc.returncode)


def cargo(env: dict[str, str], argv: list[str]) -> None:
    cmd = ["env", *[f"{k}={v}" for k, v in env.items()], "cargo", *argv]
    run(cmd)


def main() -> None:
    env = {"CARGO_HOME": ".cargo_home"}

    cargo(env, ["fmt"])

    clippy_args = [
        "clippy",
        "--all-targets",
        "--all-features",
        "--",
        "-D",
        "clippy::all",
        "-W",
        "clippy::pedantic",
    ]

    # Temporary allows: re-enable one-at-a-time.
    clippy_args += [
        "-A",
        "clippy::cast_precision_loss",
        "-A",
        "clippy::cast_possible_truncation",
        "-A",
        "clippy::cast_sign_loss",
        "-A",
        "clippy::cast_lossless",
        "-A",
        "clippy::manual_let_else",
        "-A",
        "clippy::uninlined_format_args",
    ]

    # Policy: allow mixed-script identifiers (author name etc.).
    clippy_args += ["-A", "mixed_script_confusables"]

    # Perma-ignore: doc nits are not a priority in this repo.
    clippy_args += [
        "-A",
        "clippy::missing_errors_doc",
        "-A",
        "clippy::missing_panics_doc",
    ]

    # Perma-ignore: threshold lints are noise; explicit decomposition only.
    clippy_args += ["-A", "clippy::too_many_lines"]

    # Perma-ignore: too pedantic; prefer local helpers where they belong.
    clippy_args += ["-A", "clippy::items_after_statements"]

    # Perma-ignore: too pedantic; underscore bindings are fine.
    clippy_args += ["-A", "clippy::used_underscore_binding"]

    # Indefinite-ignore: keep for now; revisit float discipline.
    clippy_args += ["-A", "clippy::float_cmp"]

    # Perma-ignore: dense domain vocab; near-collisions are often intentional.
    clippy_args += ["-A", "clippy::similar_names"]

    # Perma-ignore: status units use conventional i/j/n.
    clippy_args += ["-A", "clippy::many_single_char_names"]

    # Chosen policy: we do not blanket-`#[must_use]` everything.
    clippy_args += [
        "-A",
        "clippy::must_use_candidate",
        "-A",
        "clippy::return_self_not_must_use",
    ]

    cargo(env, clippy_args)
    cargo(env, ["test"])

    # Installation is part of the fast loop, but requires access to `$HOME/.local`.
    # In sandboxed environments it may fail (permission denied); ignore in that case.
    try:
        cargo(env, ["install", "--path", ".", "--root", os.path.expanduser("~/.local")])
    except SystemExit as e:
        if e.code != 101:
            raise


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        raise SystemExit(130)
