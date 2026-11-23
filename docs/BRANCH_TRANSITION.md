# Repository branches

GitHub's default branch is `rust`. Fresh clones check out the native Rust developer preview, and GitHub suggests `rust` as the base for new pull requests.

| Branch | Purpose | Clone command |
| --- | --- | --- |
| `rust` | Native server and workspace. Target for active development and Rust pull requests. | `git clone --branch rust https://github.com/leafall903/ORPHEUS.git` |
| `main` | Unmaintained Python server with launchers, extractors and reference tests. | `git clone --branch main https://github.com/leafall903/ORPHEUS.git` |

To update an existing clone's remote default pointer, run `git remote set-head origin -a`. This preserves the upstreams configured for existing local branches.

Run `git ls-remote --symref origin HEAD refs/heads/main refs/heads/rust` to verify the remote default and both named branches. See the [feature status](../README.md#current-status) for current gameplay and original-client evidence.
