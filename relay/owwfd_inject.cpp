// Handle-bound Windows/Wine relay injector for ORPHEUS Proton sessions.
//
// Build on Linux (MinGW-w64):
//   x86_64-w64-mingw32-g++ -O2 -Wall -Wextra -municode -static -o owwfd_inject.exe relay/owwfd_inject.cpp
// Build on Windows (MSVC x64 Native Tools):
//   cl /nologo /O2 /W3 /EHsc /DUNICODE /D_UNICODE relay\owwfd_inject.cpp /Fe:owwfd_inject.exe

#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <tlhelp32.h>

#include <cerrno>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cwctype>
#include <string>
#include <vector>

namespace {

constexpr DWORD kProcessInject =
    PROCESS_CREATE_THREAD | PROCESS_VM_OPERATION | PROCESS_VM_READ | PROCESS_VM_WRITE |
    PROCESS_QUERY_INFORMATION;
constexpr DWORD kBoundAccess = kProcessInject | SYNCHRONIZE;

std::string to_utf8(const std::wstring& w) {
    if (w.empty()) return {};
    int needed = WideCharToMultiByte(CP_UTF8, 0, w.c_str(), static_cast<int>(w.size()),
                                     nullptr, 0, nullptr, nullptr);
    if (needed <= 0) return {};
    std::string out(static_cast<size_t>(needed), '\0');
    WideCharToMultiByte(CP_UTF8, 0, w.c_str(), static_cast<int>(w.size()),
                        &out[0], needed, nullptr, nullptr);
    return out;
}

std::wstring to_lower_ascii_and_norm(const std::wstring& s) {
    wchar_t full[32768] = {};
    DWORD len = GetFullPathNameW(s.c_str(), 32768, full, nullptr);
    std::wstring norm = (len > 0 && len < 32768) ? std::wstring(full, len) : s;
    for (wchar_t& ch : norm) {
        if (ch == L'/') ch = L'\\';
        ch = static_cast<wchar_t>(towlower(ch));
    }
    return norm;
}

bool eq_icase(const std::wstring& a, const std::wstring& b) {
    if (a.size() != b.size()) return false;
    for (size_t i = 0; i < a.size(); ++i) {
        if (towlower(a[i]) != towlower(b[i])) return false;
    }
    return true;
}

struct ScopedHandle {
    HANDLE h = nullptr;
    ScopedHandle() = default;
    explicit ScopedHandle(HANDLE handle) : h(handle) {}
    ~ScopedHandle() { close(); }
    ScopedHandle(const ScopedHandle&) = delete;
    ScopedHandle& operator=(const ScopedHandle&) = delete;
    ScopedHandle(ScopedHandle&& other) noexcept : h(other.h) { other.h = nullptr; }
    ScopedHandle& operator=(ScopedHandle&& other) noexcept {
        if (this != &other) {
            close();
            h = other.h;
            other.h = nullptr;
        }
        return *this;
    }
    void close() {
        if (h && h != INVALID_HANDLE_VALUE) {
            CloseHandle(h);
        }
        h = nullptr;
    }
    bool valid() const { return h && h != INVALID_HANDLE_VALUE; }
};

struct ModuleInfo {
    std::wstring path;
    uintptr_t base = 0;
};

class BoundProcess {
public:
    DWORD pid = 0;
    ScopedHandle handle;

    static bool open(DWORD target_pid, BoundProcess& out, std::string& err) {
        if (target_pid == 0) {
            err = "PID must be positive";
            return false;
        }
        HANDLE raw = OpenProcess(kBoundAccess, FALSE, target_pid);
        if (!raw) {
            err = "OpenProcess failed (winerror=" + std::to_string(GetLastError()) + ")";
            return false;
        }
        out.pid = target_pid;
        out.handle = ScopedHandle(raw);
        bool alive = false;
        if (!out.is_alive(alive, err)) {
            out.handle.close();
            return false;
        }
        if (!alive) {
            out.handle.close();
            err = "bound process PID " + std::to_string(target_pid) + " exited before binding";
            return false;
        }
        return true;
    }

    bool is_alive(bool& alive, std::string& err) const {
        if (!handle.valid()) {
            err = "process handle is closed";
            return false;
        }
        DWORD wait = WaitForSingleObject(handle.h, 0);
        if (wait == WAIT_TIMEOUT) {
            alive = true;
            return true;
        }
        if (wait == WAIT_OBJECT_0) {
            alive = false;
            return true;
        }
        err = "WaitForSingleObject on process handle failed (winerror=" +
              std::to_string(GetLastError()) + ")";
        return false;
    }

    bool require_alive(const char* stage, std::string& err) const {
        bool alive = false;
        if (!is_alive(alive, err)) return false;
        if (!alive) {
            err = "bound process PID " + std::to_string(pid) + " exited " + stage;
            return false;
        }
        return true;
    }

    bool image_path(std::wstring& out) const {
        DWORD size = 32768;
        std::wstring buf(size, L'\0');
        if (!QueryFullProcessImageNameW(handle.h, 0, &buf[0], &size)) {
            return false;
        }
        buf.resize(size);
        out = std::move(buf);
        return true;
    }

    bool creation_time(uint64_t& out) const {
        FILETIME created{}, exited{}, kernel{}, user{};
        if (!GetProcessTimes(handle.h, &created, &exited, &kernel, &user)) {
            return false;
        }
        out = (static_cast<uint64_t>(created.dwHighDateTime) << 32) |
              static_cast<uint64_t>(created.dwLowDateTime);
        return true;
    }

    bool modules(std::vector<ModuleInfo>& out, std::string& err) const {
        if (!require_alive("before module snapshot", err)) return false;
        const ULONGLONG deadline = GetTickCount64() + 2000;
        while (true) {
            out.clear();
            ScopedHandle snap(CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid));
            if (!snap.valid()) {
                DWORD code = GetLastError();
                if ((code == ERROR_BAD_LENGTH || code == ERROR_NO_MORE_FILES ||
                     code == ERROR_PARTIAL_COPY) &&
                    GetTickCount64() < deadline) {
                    Sleep(10);
                    continue;
                }
                err = "CreateToolhelp32Snapshot(module) failed (winerror=" +
                      std::to_string(code) + ")";
                return false;
            }
            MODULEENTRY32W entry{};
            entry.dwSize = sizeof(entry);
            if (!Module32FirstW(snap.h, &entry)) {
                DWORD code = GetLastError();
                if ((code == ERROR_BAD_LENGTH || code == ERROR_NO_MORE_FILES ||
                     code == ERROR_PARTIAL_COPY) &&
                    GetTickCount64() < deadline) {
                    Sleep(10);
                    continue;
                }
                err = "Module32FirstW failed (winerror=" + std::to_string(code) + ")";
                return false;
            }
            bool ok = true;
            while (true) {
                out.push_back({entry.szExePath, reinterpret_cast<uintptr_t>(entry.modBaseAddr)});
                if (!Module32NextW(snap.h, &entry)) {
                    DWORD code = GetLastError();
                    if (code != ERROR_NO_MORE_FILES) {
                        err = "Module32NextW failed (winerror=" + std::to_string(code) + ")";
                        ok = false;
                    }
                    break;
                }
            }
            if (!ok) return false;
            if (!require_alive("during module snapshot", err)) {
                out.clear();
                return false;
            }
            return true;
        }
    }

    bool find_loaded_module(const std::wstring& dll_full, uintptr_t& base_out, std::string& err) const {
        std::vector<ModuleInfo> mods;
        if (!modules(mods, err)) return false;
        std::wstring wanted = to_lower_ascii_and_norm(dll_full);
        base_out = 0;
        for (const auto& m : mods) {
            if (to_lower_ascii_and_norm(m.path) == wanted) {
                base_out = m.base;
                break;
            }
        }
        return true;
    }

    bool resolve_remote_load_library(uintptr_t& addr_out, std::string& err) const {
        HMODULE k32 = GetModuleHandleW(L"kernel32.dll");
        FARPROC local_fn = k32 ? GetProcAddress(k32, "LoadLibraryW") : nullptr;
        if (!local_fn) {
            err = "GetProcAddress(LoadLibraryW) failed (winerror=" +
                  std::to_string(GetLastError()) + ")";
            return false;
        }
        HMODULE owner = nullptr;
        if (!GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                reinterpret_cast<LPCWSTR>(local_fn), &owner)) {
            err = "GetModuleHandleExW failed (winerror=" + std::to_string(GetLastError()) + ")";
            return false;
        }
        wchar_t owner_path[32768] = {};
        DWORD len = GetModuleFileNameW(owner, owner_path, 32768);
        if (len == 0 || len >= 32768) {
            err = "GetModuleFileNameW failed (winerror=" + std::to_string(GetLastError()) + ")";
            return false;
        }
        std::wstring owner_str(owner_path, len);
        size_t slash = owner_str.find_last_of(L"\\/");
        std::wstring wanted_name = (slash == std::wstring::npos) ? owner_str : owner_str.substr(slash + 1);

        std::vector<ModuleInfo> mods;
        if (!modules(mods, err)) return false;
        for (const auto& m : mods) {
            size_t ms = m.path.find_last_of(L"\\/");
            std::wstring mname = (ms == std::wstring::npos) ? m.path : m.path.substr(ms + 1);
            if (eq_icase(mname, wanted_name)) {
                uintptr_t rva = reinterpret_cast<uintptr_t>(local_fn) -
                                reinterpret_cast<uintptr_t>(owner);
                addr_out = m.base + rva;
                return true;
            }
        }
        err = "loader DLL " + to_utf8(wanted_name) + " not found in target PID " +
              std::to_string(pid) + "; check process architecture";
        return false;
    }

    int inject_dll(const std::wstring& dll_full, DWORD timeout_ms, uintptr_t& base_out,
                   std::string& err) const {
        if (!find_loaded_module(dll_full, base_out, err)) return 1;
        if (base_out != 0) return 0;

        uintptr_t loader = 0;
        if (!resolve_remote_load_library(loader, err)) return 1;
        if (!require_alive("before remote allocation", err)) return 1;

        const size_t bytes = (dll_full.size() + 1) * sizeof(wchar_t);
        void* remote = VirtualAllocEx(handle.h, nullptr, bytes, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
        if (!remote) {
            err = "VirtualAllocEx failed (winerror=" + std::to_string(GetLastError()) + ")";
            return 1;
        }

        SIZE_T written = 0;
        if (!WriteProcessMemory(handle.h, remote, dll_full.c_str(), bytes, &written) ||
            written != bytes) {
            DWORD code = GetLastError();
            VirtualFreeEx(handle.h, remote, 0, MEM_RELEASE);
            err = "WriteProcessMemory failed or incomplete (winerror=" + std::to_string(code) + ")";
            return 1;
        }

        if (!require_alive("before CreateRemoteThread", err)) {
            VirtualFreeEx(handle.h, remote, 0, MEM_RELEASE);
            return 1;
        }

        ScopedHandle thread(CreateRemoteThread(
            handle.h, nullptr, 0, reinterpret_cast<LPTHREAD_START_ROUTINE>(loader), remote, 0, nullptr));
        if (!thread.valid()) {
            DWORD code = GetLastError();
            VirtualFreeEx(handle.h, remote, 0, MEM_RELEASE);
            err = "CreateRemoteThread(LoadLibraryW) failed (winerror=" + std::to_string(code) + ")";
            return 1;
        }

        DWORD wait = WaitForSingleObject(thread.h, timeout_ms);
        if (wait == WAIT_TIMEOUT) {
            // Retain remote path buffer until target exit because LoadLibraryW may still read it.
            err = "DLL loader in PID " + std::to_string(pid) +
                  " timed out; outcome unknown; remote path retained until process exit";
            return 3;
        }
        if (wait != WAIT_OBJECT_0) {
            err = "waiting for DLL loader thread in PID " + std::to_string(pid) +
                  " failed (winerror=" + std::to_string(GetLastError()) +
                  "); outcome unknown; remote path retained until process exit";
            return 3;
        }

        VirtualFreeEx(handle.h, remote, 0, MEM_RELEASE);
        DWORD exit_code = 0;
        if (!GetExitCodeThread(thread.h, &exit_code)) {
            err = "GetExitCodeThread(LoadLibraryW) failed (winerror=" +
                  std::to_string(GetLastError()) + ")";
            return 1;
        }
        if (!find_loaded_module(dll_full, base_out, err)) return 1;
        if (base_out == 0) {
            err = "DLL did not load in PID " + std::to_string(pid) +
                  " (thread exit=" + std::to_string(exit_code) + ")";
            return 1;
        }
        return 0;
    }

    int start_worker(const std::wstring& dll_full, uintptr_t remote_base, std::string& err) const {
        HMODULE ours = LoadLibraryExW(dll_full.c_str(), nullptr, DONT_RESOLVE_DLL_REFERENCES);
        if (!ours) {
            err = "LoadLibraryExW(DONT_RESOLVE_DLL_REFERENCES) failed (winerror=" +
                  std::to_string(GetLastError()) + ")";
            return 1;
        }
        FARPROC local_worker = GetProcAddress(ours, "worker");
        uintptr_t rva = local_worker
                            ? (reinterpret_cast<uintptr_t>(local_worker) -
                               reinterpret_cast<uintptr_t>(ours))
                            : 0;
        FreeLibrary(ours);
        if (!local_worker) {
            err = "export 'worker' not found in relay DLL";
            return 1;
        }

        if (!require_alive("before starting worker", err)) return 1;
        uintptr_t remote_entry = remote_base + rva;
        ScopedHandle thread(CreateRemoteThread(
            handle.h, nullptr, 0, reinterpret_cast<LPTHREAD_START_ROUTINE>(remote_entry),
            nullptr, 0, nullptr));
        if (!thread.valid()) {
            err = "CreateRemoteThread(worker) failed (winerror=" + std::to_string(GetLastError()) + ")";
            return 1;
        }

        DWORD wait = WaitForSingleObject(thread.h, 100);
        if (wait == WAIT_TIMEOUT) {
            // worker loops indefinitely when it acquires the single-start guard.
            if (!require_alive("while worker started", err)) return 1;
            return 0;
        }
        if (wait != WAIT_OBJECT_0) {
            err = "waiting for remote worker thread in PID " + std::to_string(pid) +
                  " failed (winerror=" + std::to_string(GetLastError()) + "); outcome unknown";
            return 3;
        }
        DWORD exit_code = 0;
        if (!GetExitCodeThread(thread.h, &exit_code)) {
            err = "GetExitCodeThread(worker) failed (winerror=" + std::to_string(GetLastError()) + ")";
            return 1;
        }
        if (exit_code != 0) {
            err = "remote worker exited with code " + std::to_string(exit_code);
            return 1;
        }
        if (!require_alive("after worker check", err)) return 1;
        return 0;
    }
};

struct Candidate {
    DWORD pid = 0;
    uint64_t creation_time = 0;
    std::wstring path;
};

struct Unreadable {
    DWORD pid = 0;
    std::string reason;
};

bool enumerate_candidates(const std::wstring& exe_name, const std::wstring& expected_exe,
                          std::vector<Candidate>& found, std::vector<Unreadable>& unreadable,
                          std::string& err) {
    ScopedHandle snap(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0));
    if (!snap.valid()) {
        err = "CreateToolhelp32Snapshot(process) failed (winerror=" +
              std::to_string(GetLastError()) + ")";
        return false;
    }
    PROCESSENTRY32W entry{};
    entry.dwSize = sizeof(entry);
    if (!Process32FirstW(snap.h, &entry)) {
        DWORD code = GetLastError();
        if (code == ERROR_NO_MORE_FILES) return true;
        err = "Process32FirstW failed (winerror=" + std::to_string(code) + ")";
        return false;
    }
    const std::wstring expected_norm =
        expected_exe.empty() ? std::wstring() : to_lower_ascii_and_norm(expected_exe);
    while (true) {
        if (eq_icase(entry.szExeFile, exe_name)) {
            BoundProcess proc;
            std::string open_err;
            if (!BoundProcess::open(entry.th32ProcessID, proc, open_err)) {
                if (open_err.find("exited before binding") == std::string::npos) {
                    unreadable.push_back({entry.th32ProcessID, open_err});
                }
            } else {
                std::wstring path;
                uint64_t ctime = 0;
                if (!proc.image_path(path)) {
                    unreadable.push_back({entry.th32ProcessID, "cannot query image path"});
                } else if (!expected_norm.empty() && to_lower_ascii_and_norm(path) != expected_norm) {
                    // Different executable path; skip.
                } else if (!proc.creation_time(ctime)) {
                    unreadable.push_back({entry.th32ProcessID, "cannot query creation time"});
                } else {
                    found.push_back({entry.th32ProcessID, ctime, std::move(path)});
                }
            }
        }
        if (!Process32NextW(snap.h, &entry)) {
            DWORD code = GetLastError();
            if (code != ERROR_NO_MORE_FILES) {
                err = "Process32NextW failed (winerror=" + std::to_string(code) + ")";
                return false;
            }
            break;
        }
    }
    return true;
}

bool parse_u64_w(const wchar_t* text, uint64_t& out) {
    if (!text || !*text || !iswdigit(text[0])) return false;
    errno = 0;
    wchar_t* end = nullptr;
    unsigned long long v = wcstoull(text, &end, 10);
    if (errno == ERANGE || !end || *end != L'\0') return false;
    out = static_cast<uint64_t>(v);
    return true;
}

}  // namespace

int wmain(int argc, wchar_t** argv) {
    static_assert(sizeof(void*) == 8, "owwfd_inject must be built as 64-bit (x64)");

    bool list_only = false;
    bool start_worker_flag = false;
    bool has_win_pid = false;
    bool has_ctime = false;
    DWORD win_pid = 0;
    uint64_t creation_time = 0;
    DWORD timeout_ms = 10000;
    std::wstring exe_name = L"Overwatch.exe";
    std::wstring game_exe;
    std::wstring dll_path;

    for (int i = 1; i < argc; ++i) {
        std::wstring arg = argv[i];
        if (arg == L"--list") {
            list_only = true;
            continue;
        }
        if (arg == L"--start-worker") {
            start_worker_flag = true;
            continue;
        }
        if (i + 1 >= argc) {
            std::fprintf(stderr, "Missing value for %s\n", to_utf8(arg).c_str());
            return 2;
        }
        const wchar_t* val = argv[++i];
        if (arg == L"--exe-name") {
            exe_name = val;
        } else if (arg == L"--game-exe") {
            game_exe = val;
        } else if (arg == L"--dll") {
            dll_path = val;
        } else if (arg == L"--win-pid") {
            uint64_t parsed = 0;
            if (!parse_u64_w(val, parsed) || parsed == 0 || parsed > 0xFFFFFFFFULL) {
                std::fprintf(stderr, "Invalid --win-pid\n");
                return 2;
            }
            win_pid = static_cast<DWORD>(parsed);
            has_win_pid = true;
        } else if (arg == L"--creation-time") {
            if (!parse_u64_w(val, creation_time)) {
                std::fprintf(stderr, "Invalid --creation-time\n");
                return 2;
            }
            has_ctime = true;
        } else if (arg == L"--timeout-ms") {
            uint64_t parsed = 0;
            if (!parse_u64_w(val, parsed) || parsed == 0 || parsed >= 0xFFFFFFFFULL) {
                std::fprintf(stderr, "Invalid --timeout-ms\n");
                return 2;
            }
            timeout_ms = static_cast<DWORD>(parsed);
        } else {
            std::fprintf(stderr, "Unknown option: %s\n", to_utf8(arg).c_str());
            return 2;
        }
    }

    if (list_only) {
        std::vector<Candidate> found;
        std::vector<Unreadable> unreadable;
        std::string err;
        if (!enumerate_candidates(exe_name, game_exe, found, unreadable, err)) {
            std::fprintf(stderr, "Enumeration failed: %s\n", err.c_str());
            return 1;
        }
        for (const auto& u : unreadable) {
            std::fprintf(stderr, "warning: win_pid=%lu unreadable (%s)\n",
                         static_cast<unsigned long>(u.pid), u.reason.c_str());
        }
        if (found.empty()) {
            std::fprintf(stderr, "No matching %s processes found.\n", to_utf8(exe_name).c_str());
            return 1;
        }
        for (const auto& c : found) {
            std::printf("win_pid=%lu\tcreation_time=%llu\tpath=%s\n",
                        static_cast<unsigned long>(c.pid),
                        static_cast<unsigned long long>(c.creation_time),
                        to_utf8(c.path).c_str());
        }
        return unreadable.empty() ? 0 : 1;
    }

    if (dll_path.empty() || !has_win_pid || !has_ctime || game_exe.empty()) {
        std::fprintf(
            stderr,
            "Explicit target identity is required for injection: pass --dll, --win-pid, "
            "--creation-time, and --game-exe (run with --list first to inspect candidates).\n");
        return 2;
    }

    wchar_t full_dll[32768] = {};
    DWORD dll_len = GetFullPathNameW(dll_path.c_str(), 32768, full_dll, nullptr);
    if (dll_len == 0 || dll_len >= 32768) {
        std::fprintf(stderr, "Invalid DLL path: %s\n", to_utf8(dll_path).c_str());
        return 1;
    }
    DWORD attrs = GetFileAttributesW(full_dll);
    if (attrs == INVALID_FILE_ATTRIBUTES || (attrs & FILE_ATTRIBUTE_DIRECTORY) != 0) {
        std::fprintf(stderr, "Relay DLL not found: %s\n", to_utf8(full_dll).c_str());
        return 1;
    }

    BoundProcess proc;
    std::string err;
    if (!BoundProcess::open(win_pid, proc, err)) {
        std::fprintf(stderr, "Injection failed: %s\n", err.c_str());
        return 1;
    }

    std::wstring actual_path;
    if (!proc.image_path(actual_path) ||
        to_lower_ascii_and_norm(actual_path) != to_lower_ascii_and_norm(game_exe)) {
        std::fprintf(stderr,
                     "Injection failed: process PID %lu image path '%s' does not match expected '%s'\n",
                     static_cast<unsigned long>(win_pid), to_utf8(actual_path).c_str(),
                     to_utf8(game_exe).c_str());
        return 1;
    }

    uint64_t actual_ctime = 0;
    if (!proc.creation_time(actual_ctime) || actual_ctime != creation_time) {
        std::fprintf(stderr,
                     "Injection failed: process PID %lu creation time %llu does not match expected %llu; "
                     "refusing to touch a replacement process\n",
                     static_cast<unsigned long>(win_pid),
                     static_cast<unsigned long long>(actual_ctime),
                     static_cast<unsigned long long>(creation_time));
        return 1;
    }

    uintptr_t base = 0;
    int rc = proc.inject_dll(full_dll, timeout_ms, base, err);
    if (rc != 0) {
        std::fprintf(stderr, "Injection failed: %s\n", err.c_str());
        return rc;
    }

    if (start_worker_flag) {
        rc = proc.start_worker(full_dll, base, err);
        if (rc != 0) {
            std::fprintf(stderr, "Worker start failed: %s\n", err.c_str());
            return rc;
        }
    }

    if (!proc.require_alive("after relay injection", err)) {
        std::fprintf(stderr, "Injection failed: %s\n", err.c_str());
        return 1;
    }

    std::printf(
        "Loaded %s into win_pid=%lu at base=0x%llx%s "
        "(target process alive at check; verify relay readiness in game/logs).\n",
        to_utf8(full_dll).c_str(), static_cast<unsigned long>(win_pid),
        static_cast<unsigned long long>(base),
        start_worker_flag ? " and started worker thread" : "");
    return 0;
}
