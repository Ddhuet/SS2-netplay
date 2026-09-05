param([switch]$Test)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$vsScript = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat'
if (!(Test-Path -LiteralPath $vsScript)) { throw 'Visual Studio 2022 Build Tools are required to build (not to play).' }
& $env:ComSpec /d /s /c "`"`"$vsScript`" -arch=x64 -host_arch=x64 >nul && set`"" | ForEach-Object {
    if ($_ -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process') }
}
$env:CARGO_HOME = Join-Path $projectRoot '.cargo-home'
$env:CARGO_NET_GIT_FETCH_WITH_CLI = 'true'
$env:LIBCLANG_PATH = 'C:\Program Files\LLVM\bin'
$env:RUSTFLAGS = '-l shell32'
if ($Test) {
    & cargo test --locked --manifest-path "$PSScriptRoot\Cargo.toml"
} else {
    & cargo build --locked --release --manifest-path "$PSScriptRoot\Cargo.toml" --bin SS2-Netplay
}
exit $LASTEXITCODE
