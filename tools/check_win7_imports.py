#!/usr/bin/env python3
"""Audit the native launcher's PE imports against a conservative Windows 7 SP1 baseline.

Unknown DLLs, symbols and ordinal imports fail closed, including delay imports. The
allowlist contains documented Win7-or-earlier APIs used by Rust/MinGW and the native
launcher; it is not a copy of the exports of a newer Windows SDK. Additions require
checking the API's minimum supported client and the DLL exporting it on Windows 7.

This checks static imports, architecture and subsystem version. It cannot inspect
GetProcAddress/LoadLibrary targets, transitive DLL imports, CPU instructions or runtime
behaviour. Passing this audit is not evidence of a successful Windows 7 hardware test.
"""

import argparse
import json
import re
import struct
import sys
from pathlib import Path

# Each name below exists on Win7 SP1. In particular, no API-set wildcard is allowed:
# accepting every api-ms-win-* contract would silently accept Windows 8/10 imports.
SAFE_IMPORTS = {
    "kernel32.dll": """
        AcquireSRWLockExclusive AcquireSRWLockShared AddVectoredExceptionHandler
        AllocConsole AreFileApisANSI AttachConsole Beep CancelIo CancelIoEx
        CloseHandle CompareStringOrdinal ConnectNamedPipe ConvertFiberToThread
        ConvertThreadToFiber CopyFileW CreateDirectoryW CreateEventA CreateEventW
        CreateEventExW CreateFileA CreateFileW CreateFileMappingA CreateFileMappingW
        CreateHardLinkW CreateIoCompletionPort CreateMutexA CreateMutexW CreateNamedPipeW
        CreatePipe CreateProcessA CreateProcessW CreateSemaphoreW CreateSymbolicLinkW
        CreateThread CreateTimerQueueTimer CreateWaitableTimerW DebugBreak
        DecodePointer DeleteCriticalSection DeleteFileA DeleteFileW DeleteTimerQueueTimer
        DeviceIoControl DisconnectNamedPipe DuplicateHandle EncodePointer EnterCriticalSection
        EnumSystemLocalesW ExitProcess ExitThread ExpandEnvironmentStringsW
        FileTimeToLocalFileTime FileTimeToSystemTime FindClose FindFirstFileExW
        FindFirstFileW FindNextFileW FindResourceW FlsAlloc FlsFree FlsGetValue FlsSetValue
        FlushFileBuffers FlushInstructionCache FlushProcessWriteBuffers FormatMessageA
        FormatMessageW FreeConsole FreeEnvironmentStringsW FreeLibrary FreeLibraryAndExitThread
        GetACP GetCPInfo GetCommandLineA GetCommandLineW GetComputerNameW GetConsoleCP
        GetConsoleMode GetConsoleOutputCP GetConsoleScreenBufferInfo GetCurrentDirectoryA
        GetCurrentDirectoryW GetCurrentProcess GetCurrentProcessId GetCurrentThread
        GetCurrentThreadId GetDiskFreeSpaceExW GetDriveTypeW GetEnvironmentStringsW
        GetEnvironmentVariableA GetEnvironmentVariableW GetExitCodeProcess GetExitCodeThread
        GetFileAttributesA GetFileAttributesExW GetFileAttributesW GetFileInformationByHandle
        GetFileInformationByHandleEx GetFileSize GetFileSizeEx GetFileType GetFinalPathNameByHandleW
        GetFullPathNameA GetFullPathNameW GetLastError GetLocaleInfoEx GetLocaleInfoW
        GetLogicalDrives GetModuleFileNameA GetModuleFileNameW GetModuleHandleA
        GetModuleHandleW GetModuleHandleExW GetNativeSystemInfo GetNumberOfConsoleInputEvents
        GetOverlappedResult GetProcAddress GetProcessHeap GetProcessHeaps GetProcessId
        GetProcessTimes GetProcessVersion GetProcessWorkingSetSize GetStartupInfoA
        GetStartupInfoW GetStdHandle GetStringTypeW GetSystemDirectoryW GetSystemInfo
        GetSystemTime GetSystemTimeAsFileTime GetSystemTimeAdjustment GetTempFileNameW
        GetTempPathW GetThreadContext GetThreadLocale GetTickCount GetTickCount64
        GetTimeZoneInformation GetTimeZoneInformationForYear GetUserDefaultLangID GetUserDefaultLocaleName
        GetUserDefaultUILanguage GetUserPreferredUILanguages GetVersion GetVersionExA GetVersionExW GetVolumePathNameW
        GetWindowsDirectoryW GlobalAlloc GlobalFree GlobalLock GlobalMemoryStatusEx GlobalUnlock
        HeapAlloc HeapCompact HeapCreate HeapDestroy HeapFree HeapLock HeapReAlloc HeapSize
        HeapUnlock InitializeCriticalSection InitializeCriticalSectionAndSpinCount
        InitializeCriticalSectionEx InitializeSListHead InitializeSRWLock InitOnceBeginInitialize
        InitOnceComplete InitOnceExecuteOnce InterlockedFlushSList InterlockedPopEntrySList
        InterlockedPushEntrySList IsDBCSLeadByteEx IsDebuggerPresent IsProcessorFeaturePresent
        IsValidCodePage IsValidLocale IsWow64Process LCMapStringEx LCMapStringW LeaveCriticalSection
        LoadLibraryA LoadLibraryW LoadLibraryExA LoadLibraryExW LoadResource LocalAlloc LocalFree
        LocalLock LocalUnlock LockFile LockFileEx MapViewOfFile MoveFileExW MoveFileW
        MultiByteToWideChar OpenEventW OpenProcess OutputDebugStringA OutputDebugStringW
        PeekNamedPipe PostQueuedCompletionStatus Process32FirstW Process32NextW
        QueryDosDeviceW QueryFullProcessImageNameW
        QueryInformationJobObject QueryPerformanceCounter QueryPerformanceFrequency
        QueueUserWorkItem RaiseException ReadConsoleA ReadConsoleW ReadConsoleInputW
        ReadFile ReadFileEx ReadProcessMemory ReleaseMutex ReleaseSRWLockExclusive
        ReleaseSRWLockShared ReleaseSemaphore RemoveDirectoryW RemoveVectoredExceptionHandler
        ResetEvent ResumeThread RtlCaptureContext RtlLookupFunctionEntry RtlUnwindEx
        RtlVirtualUnwind SearchPathW SetConsoleCtrlHandler SetConsoleMode SetConsoleOutputCP
        SetConsoleTextAttribute SetCurrentDirectoryW SetEndOfFile SetEnvironmentVariableA
        SetEnvironmentVariableW SetErrorMode SetEvent SetFileAttributesW SetFileInformationByHandle
        SetFilePointer SetFilePointerEx SetFileTime SetHandleInformation SetLastError
        SetPriorityClass SetProcessAffinityMask SetProcessWorkingSetSize SetStdHandle
        SetThreadAffinityMask SetThreadContext SetThreadErrorMode SetThreadPriority
        SetUnhandledExceptionFilter SetWaitableTimer SizeofResource Sleep SleepConditionVariableCS
        SleepConditionVariableSRW SleepEx SwitchToThread SystemTimeToFileTime
        SystemTimeToTzSpecificLocalTime TerminateProcess TlsAlloc TlsFree TlsGetValue TlsSetValue
        TryAcquireSRWLockExclusive TryAcquireSRWLockShared TryEnterCriticalSection
        UnhandledExceptionFilter UnlockFile UnlockFileEx UnmapViewOfFile VirtualAlloc
        VirtualAllocEx VirtualFree VirtualProtect VirtualQuery WaitForMultipleObjects
        WaitForMultipleObjectsEx WaitForSingleObject WaitForSingleObjectEx WaitNamedPipeW
        WakeAllConditionVariable WakeConditionVariable WideCharToMultiByte WriteConsoleA
        WriteConsoleW WriteFile WriteFileEx WriteProcessMemory lstrcmpW lstrlenW
        CopyFileExW CreateToolhelp32Snapshot CreateWaitableTimerExW DeleteProcThreadAttributeList
        GetShortPathNameW InitializeProcThreadAttributeList IsThreadAFiber Module32FirstW
        Module32NextW SetThreadStackGuarantee UpdateProcThreadAttribute __C_specific_handler
    """,
    "advapi32.dll": """
        CredDeleteW CredFree CredReadW CredWriteW CryptAcquireContextW CryptGenRandom
        CryptReleaseContext GetUserNameW GetTokenInformation OpenProcessToken RegCloseKey
        RegCreateKeyExW RegDeleteKeyW RegDeleteTreeW RegDeleteValueW RegEnumKeyExW
        RegEnumValueW RegFlushKey RegGetValueW RegOpenKeyExW RegQueryInfoKeyW
        RegQueryValueExW RegSetValueExW SystemFunction036
        ConvertStringSecurityDescriptorToSecurityDescriptorW GetFileSecurityW RegDeleteKeyExW
        SetFileSecurityW
    """,
    "bcrypt.dll": """
        BCryptCloseAlgorithmProvider BCryptCreateHash BCryptDestroyHash BCryptFinishHash
        BCryptGenRandom BCryptGetProperty BCryptHashData BCryptOpenAlgorithmProvider
    """,
    "crypt32.dll": """
        CertCloseStore CertFindCertificateInStore CertFreeCertificateContext CertOpenStore
        CryptProtectData CryptUnprotectData
    """,
    "user32.dll": """
        AdjustWindowRect AdjustWindowRectEx BeginPaint CallWindowProcW CheckDlgButton
        ClientToScreen CloseClipboard CopyImage CreateDialogParamW CreateMenu CreatePopupMenu
        CreateWindowExW DefDlgProcW DefWindowProcW DeleteMenu DestroyIcon DestroyMenu
        DestroyWindow DialogBoxParamW DispatchMessageW DrawTextW EmptyClipboard EnableWindow
        EndDialog EndPaint EnumWindows FillRect GetActiveWindow GetCapture GetClassNameW
        GetClientRect GetClipboardData GetCursorPos GetDC GetDesktopWindow GetDlgItem
        GetDlgItemTextW GetFocus GetForegroundWindow GetKeyState GetMenu GetMessageW
        GetMonitorInfoW GetParent GetScrollInfo GetSysColor GetSysColorBrush GetSystemMetrics
        GetWindow GetWindowLongPtrW GetWindowLongW GetWindowRect GetWindowTextLengthW
        GetWindowTextW GetWindowThreadProcessId InvalidateRect IsDialogMessageW IsDlgButtonChecked
        IsIconic IsWindow IsWindowEnabled IsWindowVisible IsZoomed KillTimer LoadCursorW
        LoadIconW LoadImageW MapVirtualKeyW MessageBoxW MonitorFromPoint MonitorFromWindow
        MoveWindow OpenClipboard PeekMessageW PostMessageW PostQuitMessage RedrawWindow
        RegisterClassExW RegisterClassW RegisterWindowMessageW ReleaseCapture ReleaseDC
        ScreenToClient SendMessageW SetActiveWindow SetCapture SetClipboardData SetCursor
        SetDlgItemTextW SetFocus SetForegroundWindow SetMenu SetProcessDPIAware SetScrollInfo
        SetTimer SetWindowLongPtrW SetWindowLongW SetWindowPos SetWindowTextW ShowWindow
        SystemParametersInfoW TrackPopupMenu TranslateAcceleratorW TranslateMessage
        UnregisterClassW UpdateWindow ValidateRect wsprintfW
    """,
    "gdi32.dll": """
        BitBlt CreateCompatibleBitmap CreateCompatibleDC CreateFontIndirectW CreateFontW
        CreatePen CreateSolidBrush DeleteDC DeleteObject GetDeviceCaps GetObjectW GetStockObject
        GetTextExtentPoint32W GetTextMetricsW PatBlt Rectangle SelectObject SetBkColor
        SetBkMode SetTextColor StretchBlt TextOutW
    """,
    "comctl32.dll": "InitCommonControls InitCommonControlsEx ImageList_Destroy",
    "comdlg32.dll": "GetOpenFileNameW GetSaveFileNameW",
    "shell32.dll": """
        CommandLineToArgvW DragFinish DragQueryFileW SHBrowseForFolderW SHCreateItemFromParsingName
        SHFileOperationW SHGetFolderPathW SHGetKnownFolderPath SHGetPathFromIDListW ShellExecuteExW
        ShellExecuteW
    """,
    "shlwapi.dll": "PathIsRelativeW PathRemoveFileSpecW SHCreateStreamOnFileEx StrCmpLogicalW",
    "ole32.dll": """
        CoCreateFreeThreadedMarshaler CoCreateGuid CoCreateInstance CoInitialize CoInitializeEx
        CoTaskMemAlloc CoTaskMemFree CoTaskMemRealloc CoUninitialize
    """,
    "oleaut32.dll": "GetErrorInfo SetErrorInfo SysAllocString SysAllocStringLen SysFreeString SysStringLen",
    "userenv.dll": "GetUserProfileDirectoryW",
    "ws2_32.dll": """
        FreeAddrInfoW GetAddrInfoW WSAGetLastError WSAIoctl WSARecv WSARecvFrom WSASend WSASendTo
        WSASetLastError WSASocketW WSAStartup WSACleanup accept bind closesocket connect freeaddrinfo
        getaddrinfo gethostname getpeername getsockname getsockopt htonl htons ioctlsocket listen
        ntohl ntohs recv recvfrom select send sendto setsockopt shutdown socket
        WSADuplicateSocketW WSAPoll WSASendMsg
    """,
    "ntdll.dll": """
        NtCreateFile NtCreateNamedPipeFile NtOpenFile NtReadFile NtWriteFile NtSetInformationFile
        NtQueryInformationFile NtQueryVolumeInformationFile
        RtlGetVersion RtlNtStatusToDosError RtlCaptureContext RtlLookupFunctionEntry RtlUnwindEx
        RtlVirtualUnwind RtlAddFunctionTable RtlDeleteFunctionTable
    """,
    "iphlpapi.dll": "GetAdaptersAddresses GetAdaptersInfo GetNetworkParams CancelMibChangeNotify2 NotifyIpInterfaceChange",
    "ktmw32.dll": "CommitTransaction CreateTransaction",
    "secur32.dll": "GetUserNameExW",
    "rpcrt4.dll": "UuidCreate UuidCreateSequential UuidToStringW RpcStringFreeW",
    "version.dll": "GetFileVersionInfoSizeW GetFileVersionInfoW VerQueryValueW",
    "winmm.dll": "timeBeginPeriod timeEndPeriod timeGetTime",
    "uxtheme.dll": "CloseThemeData GetThemeSysFont IsAppThemed IsThemeActive OpenThemeData SetWindowTheme",
    "msvcrt.dll": """
        __C_specific_handler __getmainargs __initenv __iob_func __set_app_type __setusermatherr
        __wgetmainargs __winitenv _acmdln _amsg_exit _cexit _commode _errno _fmode _fpreset
        _initterm _lock _onexit _unlock _wcmdln abort calloc exit fflush fprintf free fwrite
        localeconv malloc memcmp memcpy memmove memset realloc setlocale signal strchr strcmp
        strlen strncmp strtol strtoul vfprintf wcslen wcscmp
        acos asin atan cosh frexp ldexp sinh tan tanh
    """,
}
SAFE_IMPORTS = {dll: frozenset(names.split()) for dll, names in SAFE_IMPORTS.items()}
FORBIDDEN_APIS = frozenset("""
    CreateDXGIFactory2 GetDpiForWindow GetDpiForMonitor GetSystemTimePreciseAsFileTime
    GetTempPath2W IsWow64Process2 ProcessPrng SetProcessDpiAwareness
    SetProcessDpiAwarenessContext SetThreadDescription GetThreadDescription
    SetThreadDpiAwarenessContext WaitOnAddress WakeByAddressAll WakeByAddressSingle
    GetOverlappedResultEx GetSystemTimePreciseAsFileTime DiscardVirtualMemory
    OfferVirtualMemory ReclaimVirtualMemory VirtualAlloc2 MapViewOfFile3
""".split())
MACHINES = {"x64": (0x8664, 0x20B), "x86": (0x14C, 0x10B)}


class PEError(ValueError):
    """Malformed or unsupported PE input."""


class PE:
    def __init__(self, data):
        self.data = data
        if self.take(0, 2) != b"MZ":
            raise PEError("missing MZ signature")
        pe = self.number("<I", 0x3C)
        if pe < 64 or self.take(pe, 4) != b"PE\0\0":
            raise PEError("missing PE signature")
        self.machine, section_count = self.unpack("<HH", pe + 4)
        optional_size = self.number("<H", pe + 20)
        optional = pe + 24
        self.take(optional, optional_size)
        self.magic = self.number("<H", optional)
        if self.magic not in (0x10B, 0x20B):
            raise PEError("unknown optional-header magic")
        self.width = 8 if self.magic == 0x20B else 4
        directory_offset = 112 if self.width == 8 else 96
        if optional_size < directory_offset:
            raise PEError("short optional header")
        self.image_base = self.number("<Q" if self.width == 8 else "<I",
                                      optional + (24 if self.width == 8 else 28))
        self.os_version = self.unpack("<HH", optional + 40)
        self.subsystem_version = self.unpack("<HH", optional + 48)
        self.header_size = self.number("<I", optional + 60)
        self.subsystem = self.number("<H", optional + 68)
        count = self.number("<I", optional + directory_offset - 4)
        if count > (optional_size - directory_offset) // 8:
            raise PEError("data-directory count exceeds optional header")
        self.directories = [self.unpack("<II", optional + directory_offset + i * 8)
                            for i in range(count)]
        if not 0 < section_count <= 96:
            raise PEError("invalid section count")
        section_table = optional + optional_size
        if self.header_size < section_table + section_count * 40:
            raise PEError("section table exceeds SizeOfHeaders")
        self.take(0, self.header_size)
        self.sections = []
        for i in range(section_count):
            virtual_size, address, raw_size, raw = self.unpack("<IIII", section_table + i * 40 + 8)
            if address < self.header_size or address + max(virtual_size, raw_size) > 0x100000000:
                raise PEError("invalid section address")
            if raw_size:
                if raw < self.header_size:
                    raise PEError("section raw data overlaps headers")
                self.take(raw, raw_size)
            for old_address, old_virtual, old_raw_size, old_raw in self.sections:
                if max(address, old_address) < min(address + max(virtual_size, raw_size),
                                                   old_address + max(old_virtual, old_raw_size)):
                    raise PEError("overlapping section addresses")
                if raw_size and old_raw_size and max(raw, old_raw) < min(raw + raw_size, old_raw + old_raw_size):
                    raise PEError("overlapping section file ranges")
            self.sections.append((address, virtual_size, raw_size, raw))

    def take(self, offset, length):
        if offset < 0 or length < 0 or offset + length > len(self.data):
            raise PEError("read outside PE file")
        return self.data[offset:offset + length]

    def unpack(self, fmt, offset):
        return struct.unpack(fmt, self.take(offset, struct.calcsize(fmt)))

    def number(self, fmt, offset):
        return self.unpack(fmt, offset)[0]

    def location(self, rva, length=1):
        if rva < 0 or rva + length > 0x100000000:
            raise PEError("invalid RVA")
        if rva < self.header_size and rva + length <= self.header_size:
            return rva, self.header_size - rva
        for address, _, raw_size, raw in self.sections:
            if address <= rva and rva + length <= address + raw_size:
                return raw + rva - address, address + raw_size - rva
        raise PEError(f"RVA {rva:#x} has no file-backed range")

    def string(self, rva):
        offset, available = self.location(rva)
        end = self.data.find(b"\0", offset, offset + min(available, 4096))
        if end < 0:
            raise PEError("unterminated or oversized import name")
        try:
            text = self.data[offset:end].decode("ascii")
        except UnicodeDecodeError as error:
            raise PEError("non-ASCII import name") from error
        if not text or any(ord(char) < 33 or ord(char) > 126 for char in text):
            raise PEError("invalid import name")
        return text

    def thunks(self, rva, image_based=False):
        if not rva:
            raise PEError("import descriptor has no thunk table")
        offset, available = self.location(rva, self.width)
        result = []
        for at in range(offset, offset + available - self.width + 1, self.width):
            value = self.number("<Q" if self.width == 8 else "<I", at)
            if not value:
                return result
            if value & (1 << (self.width * 8 - 1)):
                result.append(f"#{value & 0xffff}")
            else:
                if image_based:
                    value -= self.image_base
                self.location(value, 3)  # two-byte hint, then at least a NUL-terminated name
                result.append(self.string(value + 2))
        raise PEError("unterminated import thunk table")

    def imports(self):
        imports = []
        for directory, width, delayed in ((1, 20, False), (13, 32, True)):
            if directory >= len(self.directories):
                continue
            rva, size = self.directories[directory]
            if not rva and not size:
                continue
            if not rva or size < width:
                raise PEError("invalid import-directory range")
            offset, _ = self.location(rva, size)
            terminated = False
            for at in range(offset, offset + size - width + 1, width):
                values = self.unpack("<" + "I" * (width // 4), at)
                if not any(values):
                    terminated = True
                    break
                if delayed:
                    attributes, name, _, iat, table, _, _, _ = values
                    if attributes not in (0, 1):
                        raise PEError("unsupported delay-import attributes")
                    if not attributes:
                        name -= self.image_base
                        table = (table or iat) - self.image_base
                    else:
                        table = table or iat
                else:
                    table, _, _, name, iat = values
                    table = table or iat
                dll = self.string(name).lower()
                if not re.fullmatch(r"[a-z0-9_.-]+\.dll", dll):
                    raise PEError("invalid DLL name")
                imports.append({"dll": dll, "symbols": self.thunks(table, delayed and not attributes),
                                "delayed": delayed})
            if not terminated:
                raise PEError("unterminated import descriptor table")
        return imports


def audit(data, architecture="x64"):
    pe = PE(data)
    imports = pe.imports()
    problems = []
    if (pe.machine, pe.magic) != MACHINES[architecture]:
        problems.append(f"wrong architecture: machine {pe.machine:#x}, magic {pe.magic:#x}; expected {architecture}")
    if pe.os_version > (6, 1) or pe.subsystem_version > (6, 1):
        problems.append(f"PE requires newer Windows: OS {pe.os_version}, subsystem {pe.subsystem_version}")
    if pe.subsystem not in (2, 3):
        problems.append(f"unsupported subsystem {pe.subsystem}; expected Windows GUI or console")
    for imported in imports:
        dll = imported["dll"]
        if dll not in SAFE_IMPORTS:
            problems.append(f"DLL is outside the Win7 native-launcher baseline: {dll}")
        for symbol in imported["symbols"]:
            if symbol in FORBIDDEN_APIS:
                problems.append(f"post-Win7 API: {dll}!{symbol}")
            elif symbol.startswith("#"):
                problems.append(f"ordinal import cannot be verified: {dll}!{symbol}")
            elif dll in SAFE_IMPORTS and symbol not in SAFE_IMPORTS[dll]:
                problems.append(f"API needs a Win7 export audit: {dll}!{symbol}")
    return {"architecture": architecture, "machine": pe.machine,
            "os_version": list(pe.os_version), "subsystem_version": list(pe.subsystem_version),
            "imports": imports, "problems": problems, "passed": not problems,
            "scope": "Static import audit only; Windows 7 hardware execution remains required."}


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("binaries", nargs="+")
    parser.add_argument("--architecture", choices=MACHINES, default="x64")
    parser.add_argument("--json", action="store_true", help="print machine-readable audit results")
    args = parser.parse_args()
    reports = []
    for binary in args.binaries:
        try:
            report = audit(Path(binary).read_bytes(), args.architecture)
        except (OSError, PEError) as error:
            report = {"passed": False, "problems": [str(error)]}
        report["file"] = binary
        reports.append(report)
    if args.json:
        print(json.dumps(reports, ensure_ascii=False, indent=2))
    else:
        for report in reports:
            print(f"{report['file']}: {'PASS' if report['passed'] else 'FAIL'} (static Win7 import audit)")
            for imported in report.get("imports", []):
                kind = "delay" if imported["delayed"] else "load"
                print(f"  {kind} {imported['dll']}: {', '.join(imported['symbols'])}")
            for problem in report["problems"]:
                print(f"  {problem}", file=sys.stderr)
        print("Windows 7 hardware execution remains required.")
    return 0 if all(report["passed"] for report in reports) else 1


if __name__ == "__main__":
    sys.exit(main())
