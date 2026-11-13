param([Parameter(Mandatory=$true)][string]$OutDir,
    [ValidateSet('ps_2_0','vs_2_0')][string]$Profile = 'ps_2_0',
    [ValidateSet('fog','terrain')][string]$Name = 'fog')
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$compiler = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\fxc.exe" |
    Sort-Object FullName -Descending | Select-Object -First 1
if (!$compiler) { throw 'Windows SDK fxc.exe not found' }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$source = Join-Path $PSScriptRoot "$Name.hlsl"
$binary = Join-Path $OutDir "$Name.bin"
$assembly = Join-Path $OutDir "$Name.asm"
& $compiler.FullName /nologo /T $Profile /E main /O3 /WX /Fo $binary /Fc $assembly $source
if ($LASTEXITCODE -ne 0) { throw "FXC failed: $LASTEXITCODE" }
$bytes = [IO.File]::ReadAllBytes($binary)
if ($bytes.Length % 4) { throw 'Unaligned shader bytecode' }
$words = for ($i=0; $i -lt $bytes.Length; $i+=4) { [BitConverter]::ToUInt32($bytes,$i) }
$header = if ($Profile -eq 'ps_2_0') { 'ffff0200' } else { 'fffe0200' }
if ($words[0] -ne [Convert]::ToUInt32($header,16)) { throw "Shader is not $Profile" }
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
$text = "// Generated from tools/d3d9/$Name.hlsl with Windows SDK FXC $version.`n"
$text += "pub(super) const WORDS: &[u32] = &[`n"
for ($i=0; $i -lt $program.Count; $i+=4) {
    $row = $program[$i..([Math]::Min($i+3,$program.Count-1))] | ForEach-Object { '0x{0:x8}' -f $_ }
    $text += '    ' + ($row -join ', ') + ",`n"
}
$text += "];`n"
[IO.File]::WriteAllText((Join-Path $OutDir "${Name}_program.rs"),$text,[Text.UTF8Encoding]::new($false))
"FXC: $($compiler.FullName) ($version)" | Set-Content (Join-Path $OutDir "$Name.compiler.txt")
Get-FileHash $source -Algorithm SHA256 | Format-List | Out-String | Add-Content (Join-Path $OutDir "$Name.compiler.txt")
$embedded = Join-Path $PSScriptRoot "../../crates/horizon-client-d3d9/src/${Name}_program.rs"
if (Test-Path $embedded) {
    $expected = [regex]::Matches([IO.File]::ReadAllText($embedded),'0x[0-9a-fA-F]{8}') | ForEach-Object { $_.Value }
    $actual = $program | ForEach-Object { '0x{0:x8}' -f $_ }
    if (($expected -join ',') -cne ($actual -join ',')) { throw "Embedded $Name bytecode differs from FXC output" }
}
Write-Output "Shader: $Name ($Profile)"
Write-Output $text
Get-Content $assembly
