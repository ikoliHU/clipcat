#requires -Version 5.1
<#
.SYNOPSIS
    A ClipCat rögzítőmotorjának (libobs) előállítása az OBS hivatalos portable zipjéből.
.DESCRIPTION
    Letölti az OBS Studio Windows zipjét a GitHubról, és csak a rögzítéshez szükséges fájlokat
    (~56 MB) csomagolja ki az "obs" mappába (alapból a szkript mellé): a libobs-t, az FFmpeg DLL-eket,
    a játék-/monitorrögzítés, hang, NVENC, x264 és replay buffer modulokat, valamint a shadereket.
    A lemezes pufferhez mellé teszi a Gyan-féle statikus ffmpeg.exe-t (obs\ffmpeg), SHA256-ellenőrzéssel.
    Az OBS-t nem kell telepíteni. A Windows-build (cargo tauri build) beforeBuildCommandként futtatja;
    ha a célmappában már ez a verzió van, nem csinál semmit.
.PARAMETER Version
    Az OBS verziója, amelyből a motor készül.
.PARAMETER Zip
    Már letöltött zip használata letöltés helyett.
.PARAMETER FfmpegZip
    Már letöltött ffmpeg essentials zip használata letöltés helyett.
.PARAMETER Dest
    A célmappa; a telepítő a src-tauri\obs mappát csomagolja.
.PARAMETER Force
    Akkor is újra előállítja, ha a célmappában már ez a verzió van.
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

# A lemezes puffer vágója; új verziónál a hash a GitHub release asset "digest" mezőjéből
$ffmpegVersion = '9.0.1'
$ffmpegSha256 = 'fec81ae03971d9dd4be3ebe02e263bd2ec1d789483f931bdba5f5715e65da2e9'

$dest = Assert-BundleDestination $Dest
$versionFile = Join-Path $dest 'VERSION.txt'
$versionTag = "$Version+ffmpeg-$ffmpegVersion"
if (-not $Force -and (Test-BundleCache $dest $versionTag)) {
    Write-Host "Rögzítőmotor naprakész (OBS $Version, FFmpeg $ffmpegVersion): $dest"
    exit 0
}
$modules = 'win-capture', 'win-wasapi', 'obs-ffmpeg', 'obs-nvenc', 'obs-x264'

# Relatív útvonalminták (a zip gyökeréhez képest)
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
    Write-Host "OBS $Version letöltése (~180 MB)..."
    $ProgressPreference = 'SilentlyContinue'
    Invoke-WebRequest -Uri $url -OutFile $Zip -UseBasicParsing
}

Assert-ArchiveHash $Zip $obsHashes[$Version]
$archive = [System.IO.Compression.ZipFile]::OpenRead($Zip)
try {
    # A zip gyökere az a mappa, amelyben a bin/64bit/obs.dll van
    $obsDll = $archive.Entries | Where-Object { $_.FullName -match '(^|/)bin/64bit/obs\.dll$' } | Select-Object -First 1
    if (-not $obsDll) { throw 'A zipben nincs bin/64bit/obs.dll.' }
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
    Write-Host "FFmpeg $ffmpegVersion letöltése (~110 MB)..."
    $ProgressPreference = 'SilentlyContinue'
    Invoke-WebRequest -Uri $url -OutFile $FfmpegZip -UseBasicParsing
}
Assert-ArchiveHash $FfmpegZip $ffmpegSha256

# Csak a statikus ffmpeg.exe és a licence kell (GPL, ezért a licenc is a csomagba kerül)
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
    if (-not (Test-Path (Join-Path $staging $required))) { throw "Hiányzik a zipből: $required" }
}
Set-Content -Path (Join-Path $staging 'VERSION.txt') -Value $versionTag -Encoding ASCII

# Csere csak a sikeres kicsomagolás után, hogy egy megszakadt letöltés ne tegye tönkre a meglévőt
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
Write-Host ("Rögzítőmotor kész: {0} fájl, {1:N1} MB -> {2}" -f $count, $size, $dest) -ForegroundColor Green
} finally {
    Remove-BundleTemporaryDirectory $downloadDir $env:TEMP 'clipcat-bundle-'
    Remove-BundleTemporaryDirectory $staging (Split-Path $dest) 'obs.new-'
}
