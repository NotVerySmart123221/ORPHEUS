// ow_fixes.asi: the client fixes in one plugin, one source file per fix. Each module's start function runs on its
// own thread from here. To add a fix: its .c file in build.bat's ow_fixes line, its start function below.
// Research probes (tracer, guard_probe, frame_grab, dev_console, helix_probe) stay separate opt-in plugins.
// Shared log and settings: build/ow_fixes.log, build/ow_fixes.ini.
#include "fixes.h"

DWORD WINAPI retail_menu_run(LPVOID);   // retail_menu.c: retail menus, login bypass, Alt+Tab, menu skins
DWORD WINAPI aspect_ratio_run(LPVOID);  // aspect_ratio.c: the 32:9 aspect choice
DWORD WINAPI client_feed_run(LPVOID);   // client_feed.c: the client's own position for the lobby (OW_CLIENT_SYNC)

static const struct {
    const char *name;
    LPTHREAD_START_ROUTINE run;
    int priority;
} modules[] = {
    {"retail_menu", retail_menu_run, THREAD_PRIORITY_LOWEST},
    {"aspect_ratio", aspect_ratio_run, THREAD_PRIORITY_NORMAL},
    {"client_feed", client_feed_run, THREAD_PRIORITY_NORMAL},
};

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID reserved) {
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(instance);
        for (int i = 0; i < (int)(sizeof modules / sizeof *modules); i++) {
            HANDLE thread = CreateThread(NULL, 0, modules[i].run, NULL, 0, NULL);
            if (!thread) {
                fix_log(modules[i].name, "no thread: not started");
                continue;
            }
            SetThreadPriority(thread, modules[i].priority);
            CloseHandle(thread);
        }
    }
    return TRUE;
}
