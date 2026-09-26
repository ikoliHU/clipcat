#requires -Version 5.1
<#
.SYNOPSIS
    Build the ClipCat recording engine (libobs) from the official OBS portable zip.
.DESCRIPTION
    Download the OBS Studio Windows zip from GitHub and extract only the files needed
    for recording (~56 MB) into the "obs" folder (next to this script by default): libobs,
    FFmpeg DLLs, game/monitor capture, audio, NVENC, x264, replay buffer modules, and shaders.
    Add Gyan's static ffmpeg.exe for the disk buffer (obs\ffmpeg), verified with SHA256.
    OBS does not need to be installed. The Windows build (cargo tauri build) runs this as beforeBuildCommand;
    if the destination already contains this version, no changes are made.
.PARAMETER Version
    The OBS version used to build the engine.
.PARAMETER Zip
    Use an existing zip instead of downloading it.
.PARAMETER FfmpegZip
    Use an existing ffmpeg essentials zip instead of downloading it.
.PARAMETER Dest
    The destination folder; the installer bundles src-tauri\obs.
.PARAMETER Force
    Rebuild even if the destination already contains this version.
#>
param(
    [string]$Version = '32.2.2',
    [string]$Zip,
    [string]$FfmpegZip,
    [string]$Dest = (Join-Path $PSScriptRoot 'src-tauri\obs'),
    [switch]$Force
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
. (Join-Path $PSScriptRoot 'scripts\bundle-lib.ps1')

# Official GitHub release asset digest; new versions need a reviewed pin.
$obsHashes = @{ '32.2.2' = '4d6e40e3ab155f56b30de517380566a206d74b63cdf5ad49aa596924768f97e1' }
if (-not $obsHashes.ContainsKey($Version)) { throw "No reviewed OBS SHA256 for $Version" }

# Disk buffer trimmer; for a new version, get the hash from the GitHub release asset's "digest" field
$ffmpegVersion = '9.0.1'
$ffmpegSha256 = 'fec81ae03971d9dd4be3ebe02e263bd2ec1d789483f931bdba5f5715e65da2e9'

$dest = Assert-BundleDestination $Dest
$versionFile = Join-Path $dest 'VERSION.txt'
$versionTag = "$Version+ffmpeg-$ffmpegVersion"
if (-not $Force -and (Test-BundleCache $dest $versionTag)) {
    Write-Host "Recording engine is up to date (OBS $Version, FFmpeg $ffmpegVersion): $dest"
    exit 0
}
$modules = 'win-capture', 'win-wasapi', 'obs-ffmpeg', 'obs-nvenc', 'obs-x264'

# Relative path patterns (from the zip root)
$patterns = @(
    'bin/64bit/obs.dll', 'bin/64bit/libobs-d3d11.dll', 'bin/64bit/libobs-winrt.dll',
    'bin/64bit/w32-pthreads.dll', 'bin/64bit/zlib.dll', 'bin/64bit/libcurl.dll',
    'bin/64bit/librist.dll', 'bin/64bit/srt.dll',
    'bin/64bit/obs-ffmpeg-mux.exe', 'bin/64bit/obs-nvenc-test.exe',
    'bin/64bit/avcodec-*.dll', 'bin/64bit/avdevice-*.dll', 'bin/64bit/avfilter-*.dll',
    'bin/64bit/avformat-*.dll', 'bin/64bit/avutil-*.dll', 'bin/64bit/swresample-*.dll',
    'bin/64bit/swscale-*.dll', 'bin/64bit/libx264-*.dll',
    'data/libobs/*'
) + ($modules | ForEach-Object { "obs-plugins/64bit/$_.dll"; "data/obs-plugins/$_/*" })

$downloadDir = Join-Path $env:TEMP ('clipcat-bundle-' + [guid]::NewGuid().ToString('N'))
$staging = "$dest.new-$([guid]::NewGuid().ToString('N'))"
try {
New-Item -ItemType Directory -Path $downloadDir | Out-Null
$temp = $null
if (-not $Zip) {
    $Zip = Join-Path $downloadDir "OBS-Studio-$Version-Windows-x64.zip"
    $temp = $Zip
    $url = "https://github.com/obsproject/obs-studio/releases/download/$Version/OBS-Studio-$Version-Windows-x64.zip"
    Write-Host "Downloading OBS $Version (~180 MB)..."
    $ProgressPreference = 'SilentlyContinue'
    Invoke-WebRequest -Uri $url -OutFile $Zip -UseBasicParsing
}

Assert-ArchiveHash $Zip $obsHashes[$Version]
$archive = [System.IO.Compression.ZipFile]::OpenRead($Zip)
try {
    # The zip root is the folder containing bin/64bit/obs.dll
    $obsDll = $archive.Entries | Where-Object { $_.FullName -match '(^|/)bin/64bit/obs\.dll$' } | Select-Object -First 1
    if (-not $obsDll) { throw 'The zip does not contain bin/64bit/obs.dll.' }
    $root = $obsDll.FullName.Substring(0, $obsDll.FullName.Length - 'bin/64bit/obs.dll'.Length)

    $count = 0
    foreach ($entry in $archive.Entries) {
        if (-not $entry.Name -or -not $entry.FullName.StartsWith($root)) { continue }
        $relative = $entry.FullName.Substring($root.Length)
        if (-not ($patterns | Where-Object { $relative -like $_ })) { continue }
        $target = Resolve-BundleTarget $staging $relative
        New-Item -ItemType Directory -Force (Split-Path $target) | Out-Null
        [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $target, $true)
        $count++
    }
} finally {
    $archive.Dispose()
}

$ffmpegTemp = $null
if (-not $FfmpegZip) {
    $FfmpegZip = Join-Path $downloadDir "ffmpeg-$ffmpegVersion-essentials_build.zip"
    $ffmpegTemp = $FfmpegZip
    $url = "https://github.com/GyanD/codexffmpeg/releases/download/$ffmpegVersion/ffmpeg-$ffmpegVersion-essentials_build.zip"
    Write-Host "Downloading FFmpeg $ffmpegVersion (~110 MB)..."
    $ProgressPreference = 'SilentlyContinue'
    Invoke-WebRequest -Uri $url -OutFile $FfmpegZip -UseBasicParsing
}
Assert-ArchiveHash $FfmpegZip $ffmpegSha256

# Only the static ffmpeg.exe and its license are needed (GPL, so the license is bundled too)
$archive = [System.IO.Compression.ZipFile]::OpenRead($FfmpegZip)
try {
    foreach ($entry in $archive.Entries) {
        $name = switch -Regex ($entry.FullName) {
            '^[^/]+/bin/ffmpeg\.exe$' { 'ffmpeg.exe' }
            '^[^/]+/LICENSE$' { 'LICENSE.txt' }
            default { $null }
        }
        if (-not $name) { continue }
        $target = Join-Path $staging "ffmpeg\$name"
        New-Item -ItemType Directory -Force (Split-Path $target) | Out-Null
        [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $target, $true)
        $count++
    }
} finally {
    $archive.Dispose()
}

foreach ($required in @('bin\64bit\obs.dll', 'bin\64bit\obs-ffmpeg-mux.exe', 'ffmpeg\ffmpeg.exe') + ($modules | ForEach-Object { "obs-plugins\64bit\$_.dll" })) {
    if (-not (Test-Path (Join-Path $staging $required))) { throw "Missing from zip: $required" }
}
Set-Content -Path (Join-Path $staging 'VERSION.txt') -Value $versionTag -Encoding ASCII

# Replace only after successful extraction so an interrupted download cannot damage the existing engine
$hashes = [ordered]@{}
Get-ChildItem -LiteralPath $staging -Recurse -File | ForEach-Object {
    $relative = $_.FullName.Substring($staging.Length + 1).Replace('\', '/')
    $hashes[$relative] = Get-Sha256 $_.FullName
}
$hashes | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $staging 'SHA256SUMS.json') -Encoding UTF8
$verifiedDest = Assert-BundleDestination $dest
if (Test-Path -LiteralPath $verifiedDest) { Remove-Item -LiteralPath $verifiedDest -Recurse -Force }
Move-Item -LiteralPath $staging -Destination $verifiedDest
$size = (Get-ChildItem $dest -Recurse -File | Measure-Object Length -Sum).Sum / 1MB
Write-Host ("Recording engine ready: {0} files, {1:N1} MB -> {2}" -f $count, $size, $dest) -ForegroundColor Green
} finally {
    Remove-BundleTemporaryDirectory $downloadDir $env:TEMP 'clipcat-bundle-'
    Remove-BundleTemporaryDirectory $staging (Split-Path $dest) 'obs.new-'
}
