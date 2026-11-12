param([Parameter(Mandatory=$true)][string]$OutDir)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$compiler = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\fxc.exe" |
    Sort-Object FullName -Descending | Select-Object -First 1
if (!$compiler) { throw 'Windows SDK fxc.exe not found' }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$source = Join-Path $PSScriptRoot 'fog.hlsl'
$binary = Join-Path $OutDir 'fog.ps2'
$assembly = Join-Path $OutDir 'fog.asm'
& $compiler.FullName /nologo /T ps_2_0 /E main /O3 /WX /Fo $binary /Fc $assembly $source
if ($LASTEXITCODE -ne 0) { throw "FXC failed: $LASTEXITCODE" }
$bytes = [IO.File]::ReadAllBytes($binary)
if ($bytes.Length % 4) { throw 'Unaligned shader bytecode' }
$words = for ($i=0; $i -lt $bytes.Length; $i+=4) { [BitConverter]::ToUInt32($bytes,$i) }
if ($words[0] -ne [Convert]::ToUInt32('ffff0200',16)) { throw 'Shader is not ps_2_0' }
# Remove optional compiler/CTAB comments, retaining the exact executable tokens.
$program = [Collections.Generic.List[uint32]]::new()
$program.Add($words[0])
for ($i=1; $i -lt $words.Length;) {
    $opcode = $words[$i] -band 65535
    if ($opcode -eq 65535) { $program.Add($words[$i]); break }
    if ($opcode -eq 65534) { $i += 1 + (($words[$i] -shr 16) -band 32767); continue }
    $count = 1 + (($words[$i] -shr 24) -band 15)
    for ($j=0; $j -lt $count; $j++) { $program.Add($words[$i+$j]) }
    $i += $count
}
if ($program[$program.Count-1] -ne 65535) { throw 'Missing shader END' }
$version = $compiler.VersionInfo.FileVersion
$text = "// Generated from tools/d3d9/fog.hlsl with Windows SDK FXC $version.`n"
$text += "pub(super) const WORDS: &[u32] = &[`n"
for ($i=0; $i -lt $program.Count; $i+=4) {
    $row = $program[$i..([Math]::Min($i+3,$program.Count-1))] | ForEach-Object { '0x{0:x8}' -f $_ }
    $text += '    ' + ($row -join ', ') + ",`n"
}
$text += "];`n"
[IO.File]::WriteAllText((Join-Path $OutDir 'fog_program.rs'),$text,[Text.UTF8Encoding]::new($false))
"FXC: $($compiler.FullName) ($version)" | Set-Content (Join-Path $OutDir 'compiler.txt')
Get-FileHash $source -Algorithm SHA256 | Format-List | Out-String | Add-Content (Join-Path $OutDir 'compiler.txt')
$embedded = Join-Path $PSScriptRoot '../../crates/horizon-client-d3d9/src/fog_program.rs'
if (Test-Path $embedded) {
    $expected = [regex]::Matches([IO.File]::ReadAllText($embedded),'0x[0-9a-fA-F]{8}') | ForEach-Object { $_.Value }
    $actual = $program | ForEach-Object { '0x{0:x8}' -f $_ }
    if (($expected -join ',') -cne ($actual -join ',')) { throw 'Embedded fog bytecode differs from FXC output' }
}
Get-Content $assembly
