# Win7 import compatibility patch

This is windows-link 0.2.1 from microsoft/windows-rs, retaining its MIT/Apache
licences. Only the win7 vendor target redirects CoTaskMemAlloc, CoTaskMemFree
and CoTaskMemRealloc from combase.dll to ole32.dll.

Windows-sys 0.61 metadata selects combase for Alloc/Free; old Windows bindings
also import those symbols from Ole32. Raw import library order can therefore
change the resulting EXE after an unrelated dependency/LTO change. Win7 exports
these functions in Ole32, not combase. The wrapper makes that choice explicit,
while preserving x86 name decoration and all other upstream macro behaviour.

No general COM/WinRT library substitution is allowed. Unknown/newer symbols
still fail tools/check_win7_imports.py. Do not whitelist combase.

API contracts:
- https://learn.microsoft.com/windows/win32/api/combaseapi/nf-combaseapi-cotaskmemalloc
- https://learn.microsoft.com/windows/win32/api/combaseapi/nf-combaseapi-cotaskmemfree
- https://learn.microsoft.com/windows/win32/api/combaseapi/nf-combaseapi-cotaskmemrealloc

The package build compiles/audits a tiny COM import probe before the full game.
It is never uploaded or shipped. Modern targets use the unchanged import choices.
