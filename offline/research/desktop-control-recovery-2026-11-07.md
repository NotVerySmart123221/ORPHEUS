# Desktop-control recovery — 2026-11-07

## Root cause and repair

The imported `.git` directory was owned by the sandbox account, while Codex's setup refresh ran as the normal desktop account. That account could modify files but could not update the directory's discretionary ACL. Every setup refresh failed while applying the protective deny ACE to `.git`: `READ_CONTROL | WRITE_DAC: Access is denied`, followed by `setup refresh had errors`. This blocked both ordinary shell commands and the desktop Node runtime before initialization.

The one-time repair copied Git metadata under the normal desktop account, compared SHA-256 hashes for all 3,283 files before and after copying, then swapped the directory with rollback on failure. Original metadata remains at `.fork-import-backup/git-permission-repair/original.git`. No files, history, credentials, firewall policy or sandbox configuration were removed or disabled.

Verification: normal sandboxed `git status` works; desktop Node initializes and enumerates the game; setup refresh reports `errors=[]`; protective deny ACEs are present again. Git HEAD and the pre-existing workspace-only research were preserved. OpenAI's [Windows sandbox troubleshooting](https://learn.chatgpt.com/docs/windows/windows-sandbox#troubleshooting-and-faq) identifies filesystem permissions as a possible legacy sandbox setup failure; the specific failing directory above came from local logs.

## Capture and input

Windows Graphics Capture through the desktop connector still times out on this game. Existing `tools/research/screengrab.py` captures the live 1274x720 client area through PrintWindow. Desktop connector clicks use frame-relative coordinates, while these captures are client-relative; this window's client begins 31 px below the frame top. The offset was verified against window geometry and successful field edits, not guessed from a different session.

User authorized the existing `gameinput.py` helper for held buttons. Its foreground actions retain the 30-second idle wait, 10-second visible countdown and mouse-movement cancellation. No alternative raw-input implementation was added.

## Current session

The first bounded session established local login, D.Va spawning, Boosters and Matrix input delivery. It did not establish held interception or beam cleanup. A temporary Matrix-toggle override was restored from `.fork-import-backup/desktop-verification-settings.ini` after that client closed.

User then authorized `retail_menu.asi` and requested no launch timeout. Current launcher:

```powershell
python -B tools/lobby/run.py <game-root> --asi tools/asi/build/retail_menu.asi
```

Run logs: `research/main-menu/20261107T162846.965666Z/`. Launcher confirms the ASI loaded and no time limit is set; ASI log at 12:28:50 confirms the keyboard hook was removed. Training is enabled. Only the authorized retail-menu plugin was loaded. The existing optional mannequin fixture is enabled for controlled beam checks.

Current automated suite: 309 tests pass. Live check results are recorded separately; a successful launch is not Range sign-off.

Follow-up: [actual Alt-Tab, Matrix protection and beam-lifecycle results](live-verification-2026-11-07/REPORT.md) are now available. The game remains open without an automatic close timer.
