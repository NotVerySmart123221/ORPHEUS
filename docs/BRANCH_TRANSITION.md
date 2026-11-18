# GitHub default branch change

On **November 19, 2026 at 12:00 UTC (14:00 Helsinki)**, the maintainer plans to select the existing `rust` branch as the GitHub repository default. The default controls the repository landing page, fresh clones, and the base GitHub initially suggests for new pull requests.

| Contribution | Pull request base | Clone command |
| --- | --- | --- |
| Native Rust server and workspace | `rust` | `git clone --branch rust https://github.com/leafall903/ORPHEUS.git` |
| Python server, launchers, extraction tools | `main` | `git clone --branch main https://github.com/leafall903/ORPHEUS.git` |

Both protected branches accept contributions before and after the switch. Existing local branches keep their upstreams. To refresh a clone's remote default pointer after the switch, run `git remote set-head origin -a`. Continue to select the appropriate base when opening a PR.

## Maintainer verification

1. Record the repository default, the two branch tips and their required checks before the scheduled window. Inventory current PR bases so the post-switch review can confirm them.
2. At the scheduled time, set **Default branch** to the existing `rust` branch in repository settings. Both branches continue using their existing names, protections and workflows.
3. Confirm the default in GitHub and run `git ls-remote --symref origin HEAD refs/heads/main refs/heads/rust`. Expect `HEAD` to refer to `refs/heads/rust` and both named refs to resolve. Reconcile changes to either tip with ordinary merges during the window. Check PR bases and their required checks on both branches.
4. Record the time and outcome. New visitors should see the Rust developer preview. Use the [feature status](../README.md#current-status) to assess gameplay and original-client evidence.
