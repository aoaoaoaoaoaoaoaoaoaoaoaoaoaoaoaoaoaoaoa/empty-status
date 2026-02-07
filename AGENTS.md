To save on cost, please read the content of entire files into context at once, and do not churn doing tiny tail/sed/head etc. commands.
I have modified the agent interface I am using to allow `cat` to return effectively unlimited content.
When outputting patches make sure you conform the the exact format. "Fake" or "failed" patches are very common for the tiniest error!

Local workflow helpers:

- `python3 scripts/check.py`
- `python3 scripts/install.py` (wraps `cargo install --release --path . --root ~/.local`; installs into `~/.local/bin`)
