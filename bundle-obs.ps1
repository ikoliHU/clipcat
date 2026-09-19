#requires -Version 5.1
<#
.SYNOPSIS
    A ClipCat rögzítőmotorjának (libobs) előállítása az OBS hivatalos portable zipjéből.
.DESCRIPTION
    Letölti az OBS Studio Windows zipjét a GitHubról, és csak a rögzítéshez szükséges fájlokat
    (~56 MB) csomagolja ki az "obs" mappába (alapból a szkript mellé): a libobs-t, az FFmpeg DLL-eket,
    a játék-/monitorrögzítés, hang, NVENC, x264 és replay buffer modulokat, valamint a shadereket.
    Az OBS-t nem kell telepíteni. A Windows-build (cargo tauri build) beforeBuildCommandként futtatja;
    ha a célmappában már ez a verzió van, nem csinál semmit.
.PARAMETER Version
    Az OBS verziója, amelyből a motor készül.
.PARAMETER Zip
    Már letöltött zip használata letöltés helyett.
.PARAMETER Dest
    A célmappa; a telepítő a src-tauri\obs mappát csomagolja.
.PARAMETER Force
    Akkor is újra előállítja, ha a célmappában már ez a verzió van.
#>
param(
    [string]$Version = '32.2.2',
    [string]$Zip,
    [string]$Dest = (Join-Path $PSScriptRoot 'src-tauri\obs'),
    [switch]$Force
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem

$dest = $Dest
$versionFile = Join-Path $dest 'VERSION.txt'
if (-not $Force -and (Test-Path (Join-Path $dest 'bin\64bit\obs.dll')) -and (Test-Path $versionFile) -and
    ((Get-Content $versionFile -Raw).Trim() -eq $Version)) {
    Write-Host "Rögzítőmotor naprakész (OBS $Version): $dest"
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

$temp = $null
if (-not $Zip) {
    $Zip = Join-Path $env:TEMP "OBS-Studio-$Version-Windows-x64.zip"
    $temp = $Zip
    $url = "https://github.com/obsproject/obs-studio/releases/download/$Version/OBS-Studio-$Version-Windows-x64.zip"
    Write-Host "OBS $Version letöltése (~180 MB)..."
    $ProgressPreference = 'SilentlyContinue'
    Invoke-WebRequest -Uri $url -OutFile $Zip -UseBasicParsing
}

$staging = "$dest.new"
Remove-Item $staging -Recurse -Force -ErrorAction SilentlyContinue
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
        $target = Join-Path $staging ($relative -replace '/', '\')
        New-Item -ItemType Directory -Force (Split-Path $target) | Out-Null
        [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $target, $true)
        $count++
    }
} finally {
    $archive.Dispose()
}

foreach ($required in @('bin\64bit\obs.dll', 'bin\64bit\obs-ffmpeg-mux.exe') + ($modules | ForEach-Object { "obs-plugins\64bit\$_.dll" })) {
    if (-not (Test-Path (Join-Path $staging $required))) { throw "Hiányzik a zipből: $required" }
}
Set-Content -Path (Join-Path $staging 'VERSION.txt') -Value $Version -Encoding ASCII

# Csere csak a sikeres kicsomagolás után, hogy egy megszakadt letöltés ne tegye tönkre a meglévőt
Remove-Item $dest -Recurse -Force -ErrorAction SilentlyContinue
Move-Item $staging $dest
if ($temp) { Remove-Item $temp -Force -ErrorAction SilentlyContinue }

$size = (Get-ChildItem $dest -Recurse -File | Measure-Object Length -Sum).Sum / 1MB
Write-Host ("Rögzítőmotor kész: {0} fájl, {1:N1} MB -> {2}" -f $count, $size, $dest) -ForegroundColor Green
