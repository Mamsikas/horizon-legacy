# Static import baseline

`tools/check_win7_imports.py` fails closed on unknown import names and DLLs. A
passing result covers the linked file's ordinary and delay import tables, CPU
architecture and subsystem version. It does not cover late-bound calls, CPU
dispatch, DLLs loaded later, or execution on Windows 7 hardware.

The first real cross-link exposed unconditional Win8 imports, which remain
forbidden: `combase.dll!CoTaskMemFree` and
`api-ms-win-core-winrt-error-l1-1-0.dll!RoOriginateErrorW`. The native shell uses
direct `ole32` COM declarations. `ipconfig`'s unused `computer` feature is off;
adapter enumeration and the resolver algorithm are unchanged.

Additional actual imports were checked against Win7-or-earlier documented
minimum client requirements, rather than accepting a whole newer SDK export
table:

| DLL / APIs | Available baseline / reference |
| --- | --- |
| Kernel32 process attribute lists, `CreateWaitableTimerExW`, `IsThreadAFiber`, `GetUserPreferredUILanguages`, `GetTimeZoneInformationForYear` | Vista: [process attribute list](https://learn.microsoft.com/windows/win32/api/processthreadsapi/nf-processthreadsapi-initializeprocthreadattributelist), [waitable timer](https://learn.microsoft.com/windows/win32/api/synchapi/nf-synchapi-createwaitabletimerexw), [fiber](https://learn.microsoft.com/windows/win32/api/fibersapi/nf-fibersapi-isthreadafiber), [preferred languages](https://learn.microsoft.com/windows/win32/api/winnls/nf-winnls-getuserpreferreduilanguages), [time zone](https://learn.microsoft.com/windows/win32/api/timezoneapi/nf-timezoneapi-gettimezoneinformationforyear) |
| Kernel32 `CopyFileExW`, `GetShortPathNameW`, ToolHelp module enumeration | XP or earlier: [copy](https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-copyfileexw), [short path](https://learn.microsoft.com/windows/win32/api/fileapi/nf-fileapi-getshortpathnamew), [ToolHelp](https://learn.microsoft.com/windows/win32/api/tlhelp32/nf-tlhelp32-createtoolhelp32snapshot) |
| Kernel32 `SetThreadStackGuarantee`, x64 exception handler | XP SP2 / x64 exception ABI: [stack guarantee](https://learn.microsoft.com/windows/win32/api/processthreadsapi/nf-processthreadsapi-setthreadstackguarantee), [handler](https://learn.microsoft.com/cpp/c-runtime-library/internal-crt-globals-and-functions) |
| Advapi32 security descriptors / file security / `RegDeleteKeyExW` | XP / XP x64: [descriptor](https://learn.microsoft.com/windows/win32/api/sddl/nf-sddl-convertstringsecuritydescriptortosecuritydescriptorw), [file security](https://learn.microsoft.com/windows/win32/api/securitybaseapi/nf-securitybaseapi-getfilesecurityw), [registry](https://learn.microsoft.com/windows/win32/api/winreg/nf-winreg-regdeletekeyexw) |
| WS2_32 `WSADuplicateSocketW`, `WSAPoll`, `WSASendMsg` | XP / Vista: [duplicate](https://learn.microsoft.com/windows/win32/api/winsock2/nf-winsock2-wsaduplicatesocketw), [poll](https://learn.microsoft.com/windows/win32/api/winsock2/nf-winsock2-wsapoll), [send](https://learn.microsoft.com/windows/win32/api/winsock2/nf-winsock2-wsasendmsg) |
| Iphlpapi `NotifyIpInterfaceChange`, `CancelMibChangeNotify2` | Vista: [notify](https://learn.microsoft.com/windows/win32/api/netioapi/nf-netioapi-notifyipinterfacechange), [cancel](https://learn.microsoft.com/windows/win32/api/netioapi/nf-netioapi-cancelmibchangenotify2) |
| KtmW32 `CreateTransaction`, `CommitTransaction` | Vista: [create](https://learn.microsoft.com/windows/win32/api/ktmw32/nf-ktmw32-createtransaction), [commit](https://learn.microsoft.com/windows/win32/api/ktmw32/nf-ktmw32-committransaction) |
| Ntdll `NtOpenFile`, `NtSetInformationFile`, `NtCreateNamedPipeFile` | Existing NT native I/O interface used by the Win7 Rust standard library: [open](https://learn.microsoft.com/windows/win32/api/winternl/nf-winternl-ntopenfile), [set information](https://learn.microsoft.com/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntsetinformationfile), [named pipe](https://learn.microsoft.com/windows/win32/devnotes/nt-create-named-pipe-file) |
| Msvcrt inverse/hyperbolic trig, `frexp`, `ldexp` | Legacy C runtime math exports, not UCRT-only functions: [math functions](https://learn.microsoft.com/cpp/c-runtime-library/floating-point-support) |

This list is evidence for a static audit, not a declaration that every current
dependency has been exercised on Win7. Driver behavior, dynamically resolved
APIs, TLS, filesystem edge cases, real accounts and the Java exporters still
require the hardware scenarios in `TESTING.md`.
