To save on cost, please read the content of entire files into context at once, and do not churn doing tiny tail/sed/head etc. commands.
I have modified the agent interface I am using to allow `cat` to return effectively unlimited content.
When outputting patches make sure you conform the the exact format. "Fake" or "failed" patches are very common for the tiniest error!

Git policy:

- Iterating on a feature: keep a single commit by using `git commit --amend` for fixups, so history reads "as if it was right the first time".
- Standalone bugfix sessions (not tied to a feature commit): keep a single fix commit by amending until done.

Semver policy:

- Any pushed change must update `Cargo.toml` `package.version` and create an annotated tag `vX.Y.Z`.
- Fixes: bump at least `PATCH`. Features: bump at least `MINOR`. Breaking changes: bump `MAJOR`.
- Do not push commits without a corresponding version bump + tag.

Local workflow helpers:

- `python3 scripts/check.py`
- `python3 scripts/install.py` (wraps `cargo install --release --path . --root ~/.local`; installs into `~/.local/bin`)
