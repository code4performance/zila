# Builds Zila Lang OS and boots it in QEMU.
#   .\os\run.ps1              window with the VGA screen
#   .\os\run.ps1 -Serial      no window: the shell runs on this terminal's stdin/stdout
param([switch]$Serial)
$root = Split-Path -Parent $PSScriptRoot

# the compiler: the newest of the release and debug builds, or one on the PATH
$zila = @("$root\target\release\zila-parse.exe", "$root\target\debug\zila-parse.exe") |
    Where-Object { Test-Path $_ } | Sort-Object { (Get-Item $_).LastWriteTime } -Descending | Select-Object -First 1
if (-not $zila) { $zila = (Get-Command zila-parse -ErrorAction SilentlyContinue).Source }
if (-not $zila) { throw "zila-parse not found. Build it with: cargo build --release" }

# QEMU: on the PATH or unpacked into %USERPROFILE%\qemu
$qemu = (Get-Command qemu-system-x86_64 -ErrorAction SilentlyContinue).Source
if (-not $qemu) { $qemu = "$env:USERPROFILE\qemu\qemu-system-x86_64.exe" }
if (-not (Test-Path $qemu)) { throw "QEMU not found. Install it with: winget install SoftwareFreedomConservancy.QEMU" }

& $zila --os "$root\os\boot.br" "$root\os\kernel.br" -o "$root\zila-os.img"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
if ($Serial) {
    & $qemu -drive format=raw,file="$root\zila-os.img" -serial stdio -display none -m 128 -no-reboot
} else {
    & $qemu -drive format=raw,file="$root\zila-os.img" -m 128 -no-reboot
}
