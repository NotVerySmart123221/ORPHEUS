# Repository branches

GitHub's default branch is `rust`. Fresh clones check out the native Rust server workspace, and GitHub suggests `rust` as the base for new pull requests.

| Branch | Purpose | Clone command |
| --- | --- | --- |
| `rust` | Native Rust server, Web 3D Scene Viewer, and Lua 5.4 Workshop/Mod workspace. Target for active development and pull requests. | `git clone --branch rust https://github.com/leafall903/ORPHEUS.git` |
| `python` | Archived Python reference server with legacy launchers, extractors, and reference tests. | `git clone --branch python https://github.com/leafall903/ORPHEUS.git` |

To update an existing clone's remote default pointer, run `git remote set-head origin -a`. This preserves the upstreams configured for existing local branches.

Run `git ls-remote --symref origin HEAD refs/heads/python refs/heads/rust` to verify the remote default and both named branches. See the [feature status](../README.md#current-status) for current gameplay and original-client evidence.
