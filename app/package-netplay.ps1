$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$packageRoot = Join-Path $projectRoot 'dist\SS2-Netplay'
$exePath = Join-Path $PSScriptRoot 'target\release\SS2-Netplay.exe'
if (!(Test-Path -LiteralPath $exePath)) { throw 'Run build-netplay.ps1 first.' }
# Never clear an existing player folder. Copy only known package artifacts.
New-Item -ItemType Directory -Force -Path $packageRoot, "$packageRoot\ROM", "$packageRoot\save", "$packageRoot\licenses" | Out-Null
Copy-Item -LiteralPath $exePath -Destination "$packageRoot\SS2-Netplay.exe"
Copy-Item -LiteralPath "$PSScriptRoot\PORTABLE_README.txt" -Destination "$packageRoot\READ ME.txt"
$crtRoot = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Redist\MSVC\14.44.35112\x64\Microsoft.VC143.CRT'
foreach ($dll in @('vcruntime140.dll', 'vcruntime140_1.dll', 'msvcp140.dll')) {
    Copy-Item -LiteralPath (Join-Path $crtRoot $dll) -Destination $packageRoot
}
Set-Content -LiteralPath "$packageRoot\ROM\PUT YOUR ROM HERE.txt" -Value 'Put exactly one unzipped Shining Soul II .gba file here. Both players need identical ROM bytes.'
Set-Content -LiteralPath "$packageRoot\save\YOUR SAVE LIVES HERE.txt" -Value 'No initial save is required. Your character save will be created here after saving in game. See READ ME.txt.'

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$sourcePath = Join-Path $packageRoot 'Sources.zip'
$sourceStream = [System.IO.File]::Open($sourcePath, [System.IO.FileMode]::Create)
$sourceZip = [System.IO.Compression.ZipArchive]::new($sourceStream, [System.IO.Compression.ZipArchiveMode]::Create)
try {
    foreach ($repo in @('getgud', 'mgba-rollback', 'mgba-rs', 'mgba-rs/mgba-sys/mgba')) {
        $repoRoot = Join-Path $projectRoot $repo
        $paths = & git -C $repoRoot ls-files
        foreach ($relative in $paths) {
            if ($relative -match '(^|/)(target|\.cargo-home|cinema)/|\.(gba|gb|gbc|sav)$') { continue }
            $absolute = Join-Path $repoRoot $relative
            if (Test-Path -LiteralPath $absolute -PathType Leaf) {
                [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($sourceZip, $absolute, "$repo/$relative", [System.IO.Compression.CompressionLevel]::Optimal) | Out-Null
            }
        }
        Copy-Item -LiteralPath "$repoRoot\LICENSE" -Destination (Join-Path "$packageRoot\licenses" ($repo.Replace('/', '-') + '-LICENSE.txt'))
    }
    $appFiles = Get-ChildItem -LiteralPath "$PSScriptRoot\src", "$PSScriptRoot\examples" -Recurse -File
    $appFiles += Get-Item -LiteralPath "$PSScriptRoot\Cargo.toml", "$PSScriptRoot\Cargo.lock", "$PSScriptRoot\README.md", "$PSScriptRoot\PORTABLE_README.txt", "$PSScriptRoot\build-netplay.ps1", "$PSScriptRoot\package-netplay.ps1"
    foreach ($file in $appFiles) {
        $entry = [System.IO.Path]::GetRelativePath($projectRoot, $file.FullName).Replace('\', '/')
        [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($sourceZip, $file.FullName, $entry, [System.IO.Compression.CompressionLevel]::Optimal) | Out-Null
    }
} finally { $sourceZip.Dispose(); $sourceStream.Dispose() }

# Include license text shipped with the exact fetched crate sources. This may
# include optional platform crates, but never a user ROM, save, or credential.
$crateRoots = Get-ChildItem -Path "$projectRoot\.cargo-home\registry\src\*" -Directory
foreach ($registry in $crateRoots) {
    foreach ($crate in (Get-ChildItem -LiteralPath $registry.FullName -Directory)) {
        $notices = Get-ChildItem -LiteralPath $crate.FullName -File | Where-Object { $_.Name -match '^(LICENSE|COPYING|NOTICE|COPYRIGHT)' }
        if ($notices) {
            $dest = Join-Path "$packageRoot\licenses" $crate.Name
            New-Item -ItemType Directory -Force -Path $dest | Out-Null
            foreach ($notice in $notices) { Copy-Item -LiteralPath $notice.FullName -Destination $dest }
        }
    }
}
$buildInfo = "Built UTC: $([DateTime]::UtcNow.ToString('o'))`nEXE SHA256: $((Get-FileHash -LiteralPath "$packageRoot\SS2-Netplay.exe" -Algorithm SHA256).Hash)`nPlatform: Windows x64; mGBA statically linked; local modified source in Sources.zip.`n"
Set-Content -LiteralPath "$packageRoot\BUILD.txt" -Value $buildInfo

# Use an allowlist so re-packaging after manual play cannot disclose local saves
# or certificate keys. The output ZIP always starts with empty ROM/save folders.
$archivePath = Join-Path $projectRoot 'dist\SS2-Netplay-Windows-x64.zip'
$archiveStream = [System.IO.File]::Open($archivePath, [System.IO.FileMode]::Create)
$archive = [System.IO.Compression.ZipArchive]::new($archiveStream, [System.IO.Compression.ZipArchiveMode]::Create)
try {
    $deliver = @('SS2-Netplay.exe', 'vcruntime140.dll', 'vcruntime140_1.dll', 'msvcp140.dll', 'READ ME.txt', 'BUILD.txt', 'Sources.zip', 'ROM\PUT YOUR ROM HERE.txt', 'save\YOUR SAVE LIVES HERE.txt')
    $deliver += Get-ChildItem -LiteralPath "$packageRoot\licenses" -File -Recurse | ForEach-Object { [System.IO.Path]::GetRelativePath($packageRoot, $_.FullName) }
    foreach ($relative in $deliver) {
        [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, (Join-Path $packageRoot $relative), ('SS2-Netplay/' + $relative.Replace('\', '/')), [System.IO.Compression.CompressionLevel]::Optimal) | Out-Null
    }
} finally { $archive.Dispose(); $archiveStream.Dispose() }
Get-Item -LiteralPath "$packageRoot\SS2-Netplay.exe", $archivePath | Select-Object FullName, Length
