# Builds Brasa OS and boots it in QEMU.
#   .\os\run.ps1              window with the VGA screen
#   .\os\run.ps1 -Serial      no window: the shell runs on this terminal's stdin/stdout
param([switch]$Serial)
$root = Split-Path -Parent $PSScriptRoot

# the compiler: the newest of the release and debug builds, or one on the PATH
$brasa = @("$root\target\release\brasa-parse.exe", "$root\target\debug\brasa-parse.exe") |
    Where-Object { Test-Path $_ } | Sort-Object { (Get-Item $_).LastWriteTime } -Descending | Select-Object -First 1
if (-not $brasa) { $brasa = (Get-Command brasa-parse -ErrorAction SilentlyContinue).Source }
if (-not $brasa) { throw "brasa-parse not found. Build it with: cargo build --release" }

# QEMU: on the PATH or unpacked into %USERPROFILE%\qemu
$qemu = (Get-Command qemu-system-x86_64 -ErrorAction SilentlyContinue).Source
if (-not $qemu) { $qemu = "$env:USERPROFILE\qemu\qemu-system-x86_64.exe" }
if (-not (Test-Path $qemu)) { throw "QEMU not found. Install it with: winget install SoftwareFreedomConservancy.QEMU" }

& $brasa --os "$root\os\boot.br" "$root\os\kernel.br" -o "$root\brasa-os.img"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
if ($Serial) {
    & $qemu -drive format=raw,file="$root\brasa-os.img" -serial stdio -display none -m 128 -no-reboot
} else {
    & $qemu -drive format=raw,file="$root\brasa-os.img" -m 128 -no-reboot
}
