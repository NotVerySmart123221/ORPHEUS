//! Portable Linux Proton runtime resolver and explicit relay helper runner.
//!
//! Resolves `WINEPREFIX`, active Proton tool (strictly confirmed via `/proc/<pid>/maps`),
//! and Wine sync flags from a running Linux process, then invokes the Windows x64 helper
//! (`owwfd_inject.exe`) inside that Wine session without guessing target processes.

#![forbid(unsafe_code)]

use std::process::ExitCode;

#[cfg(not(target_os = "linux"))]
fn main() -> ExitCode {
    eprintln!("orpheus_proton: this tool is only supported on Linux hosts running Steam/Proton.");
    ExitCode::FAILURE
}

#[cfg(target_os = "linux")]
fn main() -> ExitCode {
    linux_impl::main()
}

#[cfg(target_os = "linux")]
mod linux_impl {
    use std::collections::BTreeMap;
    use std::env;
    use std::ffi::OsString;
    use std::fs;
    use std::io::{self, Read};
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Component, Path, PathBuf};
    use std::process::{Command, ExitCode, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    const USAGE: &str = "Usage:
  1. List matching Windows processes in the active Proton session:
     orpheus_proton --list --inject-exe <path/to/owwfd_inject.exe> [--runtime-pid <linux-pid>]
                    [--exe-name <Overwatch.exe>] [--game-exe <windows-path>] [--timeout <secs>]

  2. Inject using the exact win_pid, creation_time, and path printed by --list:
     orpheus_proton --inject-exe <path/to/owwfd_inject.exe> [--relay-dll <relay/owwfd_relay.dll>]
                    [--runtime-pid <linux-pid>] --win-pid <pid> --creation-time <filetime>
                    --game-exe <windows-path> [--timeout <secs>] [--start-worker]

Notes:
  - --runtime-pid only resolves WINEPREFIX, active Proton runtime, and Wine sync env.
    It does NOT assert Linux-to-Windows PID identity.
  - --start-worker explicitly invokes an exported worker() entry point after LoadLibraryW
    (for relay DLL builds that export worker, such as main PR #22).
  - Set ORPHEUS_INJECT_EXE to avoid passing --inject-exe on every run.
  - Build the Windows helper on Linux with MinGW:
    x86_64-w64-mingw32-g++ -O2 -Wall -Wextra -municode -static \\
      -o owwfd_inject.exe relay/owwfd_inject.cpp";

    const SYNC_ENV_KEYS: [&str; 5] = [
        "WINEFSYNC",
        "WINEESYNC",
        "WINE_NTSYNC",
        "PROTON_ENABLE_NTSYNC",
        "WINEFUTEX2",
    ];

    struct CliArgs {
        list_only: bool,
        runtime_pid: Option<u32>,
        win_pid: Option<u32>,
        creation_time: Option<u64>,
        exe_name: String,
        game_exe: Option<String>,
        inject_exe: Option<PathBuf>,
        relay_dll: PathBuf,
        timeout_secs: f64,
        start_worker: bool,
    }

    struct ProtonRuntime {
        wine_bin: PathBuf,
        wineprefix: PathBuf,
        ld_library_path: String,
        sync_env: BTreeMap<String, String>,
    }

    struct HelperOutput {
        code: u8,
        stdout: String,
        stderr: String,
    }

    pub(super) fn main() -> ExitCode {
        match run() {
            Ok(code) => ExitCode::from(code),
            Err(err) => {
                eprintln!("orpheus_proton: {err}");
                ExitCode::FAILURE
            }
        }
    }

    fn run() -> Result<u8, String> {
        let Some(args) = parse_args()? else {
            return Ok(0);
        };

        let inject_exe = args
            .inject_exe
            .as_ref()
            .ok_or_else(|| "pass --inject-exe PATH or set ORPHEUS_INJECT_EXE".to_owned())?;

        if !args.list_only
            && (args.win_pid.is_none()
                || args.creation_time.is_none()
                || args.game_exe.as_deref().is_none_or(str::is_empty))
        {
            return Err(
                "injection requires explicit --win-pid, --creation-time, and --game-exe \
                 (run --list first; --runtime-pid only resolves the Wine environment)"
                    .to_owned(),
            );
        }

        if !(args.timeout_secs.is_finite() && args.timeout_secs > 0.0) {
            return Err(format!(
                "--timeout must be positive and finite, got {}",
                args.timeout_secs
            ));
        }
        let timeout = Duration::try_from_secs_f64(args.timeout_secs)
            .map_err(|_| format!("--timeout out of range: {}", args.timeout_secs))?;
        if Instant::now().checked_add(timeout).is_none() {
            return Err(format!(
                "--timeout overflows system clock: {}",
                args.timeout_secs
            ));
        }

        let proc_root = Path::new("/proc");
        let runtime_pid = match args.runtime_pid {
            Some(pid) => pid,
            None => {
                let pids = find_linux_game_pids(&args.exe_name, proc_root);
                match pids.as_slice() {
                    [] => {
                        return Err(format!("no running {} Linux process found", args.exe_name));
                    }
                    [single] => *single,
                    many => {
                        return Err(format!(
                            "multiple {} Linux processes found ({many:?}); pass --runtime-pid",
                            args.exe_name
                        ));
                    }
                }
            }
        };

        let runtime = resolve_proton_runtime(runtime_pid, proc_root)?;
        let out = run_proton_helper(&runtime, inject_exe, &args)?;
        if !out.stdout.is_empty() {
            print!("{}", out.stdout);
        }
        if !out.stderr.is_empty() {
            eprint!("{}", out.stderr);
        }
        Ok(out.code)
    }

    fn parse_args() -> Result<Option<CliArgs>, String> {
        let mut parsed = CliArgs {
            list_only: false,
            runtime_pid: None,
            win_pid: None,
            creation_time: None,
            exe_name: "Overwatch.exe".to_owned(),
            game_exe: None,
            inject_exe: env::var_os("ORPHEUS_INJECT_EXE").map(PathBuf::from),
            relay_dll: PathBuf::from("relay/owwfd_relay.dll"),
            timeout_secs: 30.0,
            start_worker: false,
        };

        let mut args = env::args_os().skip(1);
        while let Some(arg) = args.next() {
            if arg == "--help" || arg == "-h" {
                println!("{USAGE}");
                return Ok(None);
            }
            if arg == "--list" {
                parsed.list_only = true;
                continue;
            }
            if arg == "--start-worker" {
                parsed.start_worker = true;
                continue;
            }
            let value = args
                .next()
                .ok_or_else(|| format!("missing value for option {}", arg.to_string_lossy()))?;
            match arg.to_str() {
                Some("--runtime-pid") => {
                    parsed.runtime_pid = Some(parse_u32("--runtime-pid", value)?)
                }
                Some("--win-pid") => parsed.win_pid = Some(parse_u32("--win-pid", value)?),
                Some("--creation-time") => {
                    parsed.creation_time = Some(parse_u64("--creation-time", value)?);
                }
                Some("--exe-name") => parsed.exe_name = parse_utf8("--exe-name", value)?,
                Some("--game-exe") => parsed.game_exe = Some(parse_utf8("--game-exe", value)?),
                Some("--inject-exe") => parsed.inject_exe = Some(PathBuf::from(value)),
                Some("--relay-dll") => parsed.relay_dll = PathBuf::from(value),
                Some("--timeout") => parsed.timeout_secs = parse_f64("--timeout", value)?,
                _ => return Err(format!("unknown option {}", arg.to_string_lossy())),
            }
        }
        Ok(Some(parsed))
    }

    fn parse_utf8(flag: &str, value: OsString) -> Result<String, String> {
        value
            .into_string()
            .map_err(|_| format!("invalid UTF-8 value for {flag}"))
    }

    fn parse_u32(flag: &str, value: OsString) -> Result<u32, String> {
        let text = parse_utf8(flag, value)?;
        let n: u32 = text
            .parse()
            .map_err(|_| format!("invalid integer for {flag}: {text:?}"))?;
        if n == 0 {
            return Err(format!("{flag} must be positive"));
        }
        Ok(n)
    }

    fn parse_u64(flag: &str, value: OsString) -> Result<u64, String> {
        let text = parse_utf8(flag, value)?;
        text.parse()
            .map_err(|_| format!("invalid integer for {flag}: {text:?}"))
    }

    fn parse_f64(flag: &str, value: OsString) -> Result<f64, String> {
        let text = parse_utf8(flag, value)?;
        text.parse()
            .map_err(|_| format!("invalid number for {flag}: {text:?}"))
    }

    fn find_linux_game_pids(exe_name: &str, proc_root: &Path) -> Vec<u32> {
        let wanted = exe_name.to_ascii_lowercase();
        let Ok(entries) = fs::read_dir(proc_root) else {
            return Vec::new();
        };
        let mut pids = Vec::new();
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(pid) = name.parse::<u32>() else {
                continue;
            };
            let pdir = entry.path();
            if let Ok(comm) = fs::read_to_string(pdir.join("comm")) {
                if comm.trim().to_ascii_lowercase() == wanted {
                    pids.push(pid);
                    continue;
                }
            }
            if let Ok(cmdline) = fs::read(pdir.join("cmdline")) {
                if let Some(first) = cmdline.split(|b| *b == 0).next() {
                    if !first.is_empty() {
                        let argv0 = String::from_utf8_lossy(first).replace('\\', "/");
                        if Path::new(&argv0)
                            .file_name()
                            .and_then(|s| s.to_str())
                            .is_some_and(|base| base.to_ascii_lowercase() == wanted)
                        {
                            pids.push(pid);
                        }
                    }
                }
            }
        }
        pids.sort_unstable();
        pids
    }

    fn read_proc_environ(pid: u32, proc_root: &Path) -> Result<BTreeMap<String, String>, String> {
        let path = proc_root.join(pid.to_string()).join("environ");
        let raw = fs::read(&path)
            .map_err(|err| format!("cannot read environment of Linux PID {pid}: {err}"))?;
        let mut env_map = BTreeMap::new();
        for entry in raw.split(|b| *b == 0) {
            if entry.is_empty() {
                continue;
            }
            if let Some(eq) = entry.iter().position(|b| *b == b'=') {
                let key = String::from_utf8_lossy(&entry[..eq]).into_owned();
                let val = String::from_utf8_lossy(&entry[eq + 1..]).into_owned();
                env_map.insert(key, val);
            }
        }
        Ok(env_map)
    }

    fn maps_confirm_tool_files(pid: u32, files_dir: &Path, proc_root: &Path) -> bool {
        let path = proc_root.join(pid.to_string()).join("maps");
        let Ok(maps_text) = fs::read_to_string(path) else {
            return false;
        };
        let prefix = format!("{}/", files_dir.display());
        for line in maps_text.lines() {
            let Some(mapped) = maps_pathname(line) else {
                continue;
            };
            let cleaned = mapped.strip_suffix(" (deleted)").unwrap_or(mapped);
            if cleaned.starts_with(&prefix) {
                return true;
            }
        }
        false
    }

    fn maps_pathname(line: &str) -> Option<&str> {
        // /proc/<pid>/maps has 5 whitespace-separated fields before the optional pathname column.
        let mut rest = line.trim_start();
        for _ in 0..5 {
            let split_at = rest.find(char::is_whitespace)?;
            rest = rest[split_at..].trim_start();
        }
        if rest.is_empty() {
            None
        } else {
            Some(rest.trim_end())
        }
    }

    fn is_executable_file(path: &Path) -> bool {
        fs::metadata(path)
            .is_ok_and(|meta| meta.is_file() && (meta.permissions().mode() & 0o111) != 0)
    }

    fn select_tool_dir(pid: u32, compat_paths: &str, proc_root: &Path) -> Result<PathBuf, String> {
        let mut candidates = Vec::new();
        for raw in compat_paths.split(':') {
            if raw.is_empty() {
                continue;
            }
            let tool = Path::new(raw);
            if !tool.is_absolute() {
                return Err(format!(
                    "relative path {raw:?} in STEAM_COMPAT_TOOL_PATHS for Linux PID {pid}; refusing to guess"
                ));
            }
            let wine_bin = tool.join("files/bin/wine");
            if is_executable_file(&wine_bin) {
                let resolved = fs::canonicalize(tool).map_err(|err| {
                    format!("cannot canonicalize Proton tool {raw:?} for PID {pid}: {err}")
                })?;
                if !candidates.contains(&resolved) {
                    candidates.push(resolved);
                }
            }
        }
        if candidates.is_empty() {
            return Err(format!(
                "no Proton runtime with files/bin/wine found in STEAM_COMPAT_TOOL_PATHS for PID {pid}"
            ));
        }

        let mapped: Vec<PathBuf> = candidates
            .iter()
            .filter(|tool| maps_confirm_tool_files(pid, &tool.join("files"), proc_root))
            .cloned()
            .collect();

        match mapped.as_slice() {
            [single] => Ok(single.clone()),
            [] => Err(format!(
                "none of the STEAM_COMPAT_TOOL_PATHS candidates are mapped in /proc/{pid}/maps: {}",
                join_paths(&candidates)
            )),
            many => Err(format!(
                "ambiguous Proton runtime for PID {pid}; multiple mapped candidates: {}",
                join_paths(many)
            )),
        }
    }

    fn join_paths(paths: &[PathBuf]) -> String {
        paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn resolve_proton_runtime(pid: u32, proc_root: &Path) -> Result<ProtonRuntime, String> {
        let env_map = read_proc_environ(pid, proc_root)?;
        let raw_prefix = env_map
            .get("WINEPREFIX")
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("WINEPREFIX is missing in environment of Linux PID {pid}"))?;
        if !Path::new(raw_prefix).is_absolute() {
            return Err(format!(
                "relative WINEPREFIX {raw_prefix:?} for Linux PID {pid}; refusing to guess"
            ));
        }
        let wineprefix = fs::canonicalize(raw_prefix).map_err(|err| {
            format!("WINEPREFIX {raw_prefix:?} for Linux PID {pid} is invalid: {err}")
        })?;
        if !wineprefix.is_dir() {
            return Err(format!(
                "WINEPREFIX {} for Linux PID {pid} is not a directory",
                wineprefix.display()
            ));
        }

        let compat_paths = env_map
            .get("STEAM_COMPAT_TOOL_PATHS")
            .map(String::as_str)
            .unwrap_or("");
        let tool_dir = select_tool_dir(pid, compat_paths, proc_root)?;
        let files_dir = tool_dir.join("files");
        let wine_bin = files_dir.join("bin/wine");

        let mut sync_env = BTreeMap::new();
        for key in SYNC_ENV_KEYS {
            if let Some(value) = env_map.get(key) {
                if value != "0" && value != "1" {
                    return Err(format!(
                        "unexpected {key}={value:?} in Linux PID {pid}; expected '0' or '1'"
                    ));
                }
                sync_env.insert(key.to_owned(), value.clone());
            }
        }

        let mut lib_dirs = Vec::new();
        for rel in ["lib64", "lib", "lib/x86_64-linux-gnu"] {
            let dir = files_dir.join(rel);
            if dir.is_dir() {
                lib_dirs.push(dir.display().to_string());
            }
        }

        Ok(ProtonRuntime {
            wine_bin,
            wineprefix,
            ld_library_path: lib_dirs.join(":"),
            sync_env,
        })
    }

    fn to_wine_path(path: &Path) -> Result<String, String> {
        let resolved = fs::canonicalize(path)
            .map_err(|err| format!("cannot resolve path {}: {err}", path.display()))?;
        let mut parts = Vec::new();
        for comp in resolved.components() {
            match comp {
                Component::RootDir => {}
                Component::Normal(seg) => {
                    let s = seg.to_str().ok_or_else(|| {
                        format!("non-UTF-8 path component in {}", resolved.display())
                    })?;
                    parts.push(s);
                }
                _ => {
                    return Err(format!(
                        "unexpected path component in canonical path {}",
                        resolved.display()
                    ));
                }
            }
        }
        Ok(format!("Z:\\{}", parts.join("\\")))
    }

    fn run_proton_helper(
        runtime: &ProtonRuntime,
        inject_exe: &Path,
        args: &CliArgs,
    ) -> Result<HelperOutput, String> {
        if !inject_exe.is_file() {
            return Err(format!(
                "Windows helper executable not found: {}",
                inject_exe.display()
            ));
        }
        let inject_canon = fs::canonicalize(inject_exe).map_err(|err| {
            format!(
                "cannot resolve Windows helper {}: {err}",
                inject_exe.display()
            )
        })?;
        let inject_win = to_wine_path(&inject_canon)?;

        let mut cmd = Command::new(&runtime.wine_bin);
        cmd.arg(&inject_win);
        cmd.arg("--exe-name").arg(&args.exe_name);

        if args.list_only {
            cmd.arg("--list");
        } else {
            if !args.relay_dll.is_file() {
                return Err(format!("relay DLL not found: {}", args.relay_dll.display()));
            }
            let dll_win = to_wine_path(&args.relay_dll)?;
            cmd.arg("--dll").arg(dll_win);
        }
        if let Some(win_pid) = args.win_pid {
            cmd.arg("--win-pid").arg(win_pid.to_string());
        }
        if let Some(ctime) = args.creation_time {
            cmd.arg("--creation-time").arg(ctime.to_string());
        }
        if let Some(game_exe) = &args.game_exe {
            cmd.arg("--game-exe").arg(game_exe);
        }
        if args.start_worker {
            cmd.arg("--start-worker");
        }

        for key in SYNC_ENV_KEYS {
            cmd.env_remove(key);
        }
        for (key, value) in &runtime.sync_env {
            cmd.env(key, value);
        }
        cmd.env("WINEPREFIX", &runtime.wineprefix);
        if !runtime.ld_library_path.is_empty() {
            cmd.env("LD_LIBRARY_PATH", &runtime.ld_library_path);
        }
        if let Some(parent) = inject_canon.parent().filter(|p| !p.as_os_str().is_empty()) {
            cmd.current_dir(parent);
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|err| {
            format!(
                "failed to start Proton helper ({}): {err}",
                runtime.wine_bin.display()
            )
        })?;

        let stdout_handle = child.stdout.take().map(spawn_reader);
        let stderr_handle = child.stderr.take().map(spawn_reader);

        let timeout = Duration::try_from_secs_f64(args.timeout_secs)
            .map_err(|_| format!("--timeout out of range: {}", args.timeout_secs))?;
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| format!("--timeout overflows system clock: {}", args.timeout_secs))?;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(format!(
                            "Proton helper timed out after {:.1}s; outcome unknown \
                             (existing Wine session and wineserver left untouched)",
                            args.timeout_secs
                        ));
                    }
                    thread::sleep(Duration::from_millis(25));
                }
                Err(err) => {
                    return Err(format!(
                        "failed while waiting for Proton helper: {err}; outcome unknown"
                    ));
                }
            }
        };

        let stdout = join_reader(stdout_handle);
        let stderr = join_reader(stderr_handle);
        let code = status
            .code()
            .and_then(|c| u8::try_from(c).ok())
            .unwrap_or(1);

        Ok(HelperOutput {
            code,
            stdout,
            stderr,
        })
    }

    fn spawn_reader<R: Read + Send + 'static>(mut reader: R) -> thread::JoinHandle<String> {
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = io::Read::read_to_end(&mut reader, &mut buf);
            String::from_utf8_lossy(&buf).into_owned()
        })
    }

    fn join_reader(handle: Option<thread::JoinHandle<String>>) -> String {
        handle.and_then(|h| h.join().ok()).unwrap_or_default()
    }
}
