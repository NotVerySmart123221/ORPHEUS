@echo off
rem Builds the ASI plugins with MSVC 2022 x64.
call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul || exit /b 1
cd /d "%~dp0"
if not exist build mkdir build
rem ow_fixes.asi: every client fix, one module per source file (ow_fixes.c lists them). Self-checks first.
cl /nologo /O2 /W4 test_aspect_ratio.c /Fe:build\test_aspect_ratio.exe /Fo:build\ /link /NOLOGO user32.lib || exit /b 1
build\test_aspect_ratio.exe || exit /b 1
cl /nologo /O2 /W3 test_retail_menu.c /Fe:build\test_retail_menu.exe /Fo:build\ /link /NOLOGO user32.lib || exit /b 1
build\test_retail_menu.exe || exit /b 1
cl /nologo /O2 /W4 test_client_feed.c /Fe:build\test_client_feed.exe /Fo:build\ /link /NOLOGO ws2_32.lib || exit /b 1
build\test_client_feed.exe || exit /b 1
cl /nologo /O2 /LD /W3 ow_fixes.c retail_menu.c aspect_ratio.c client_feed.c /Fe:build\ow_fixes.asi /Fo:build\ /link /NOLOGO user32.lib ws2_32.lib || exit /b 1
rem retail_menu.asi and aspect_ratio.asi are modules of ow_fixes.asi now; old copies loaded beside it would hook twice.
del /q build\retail_menu.asi build\aspect_ratio.asi 2>nul
cl /nologo /O2 /LD /W3 helix_probe.c /Fe:build\helix_probe.asi /Fo:build\ /link /NOLOGO || exit /b 1
cl /nologo /O2 /LD /W3 tracer.c /Fe:build\tracer.asi /Fo:build\ /link /NOLOGO || exit /b 1
cl /nologo /O2 /LD /W3 guard_probe.c /Fe:build\guard_probe.asi /Fo:build\ /link /NOLOGO || exit /b 1
cl /nologo /O2 /LD /W3 frame_grab.c /Fe:build\frame_grab.asi /Fo:build\ /link /NOLOGO user32.lib d3d11.lib dxgi.lib dxguid.lib || exit /b 1
cl /nologo /O2 /LD /W3 dev_console.c /Fe:build\dev_console.asi /Fo:build\ /link /NOLOGO user32.lib || exit /b 1
