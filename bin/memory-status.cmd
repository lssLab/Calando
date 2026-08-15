@echo off
set "MEMORY_SUPERVISOR_BIN=%MEMORY_SUPERVISOR_BINARY%"
if not defined MEMORY_SUPERVISOR_BIN if exist "%USERPROFILE%\.memory-supervisor\binary" set /p MEMORY_SUPERVISOR_BIN=<"%USERPROFILE%\.memory-supervisor\binary"
if not defined MEMORY_SUPERVISOR_BIN set "MEMORY_SUPERVISOR_BIN=%USERPROFILE%\.local\lib\memory-supervisor\memory-supervisor.exe"
"%MEMORY_SUPERVISOR_BIN%" status %*
exit /b %errorlevel%
