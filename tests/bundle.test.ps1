$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '..\scripts\bundle-lib.ps1')
$root = Join-Path ([System.IO.Path]::GetTempPath()) ('clipcat-zip-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root | Out-Null
try {
    foreach ($bad in @('data/libobs/../../../../outside.txt', '../x', '/outside', 'C:\outside', 'data\..\x', 'data/file:stream')) {
        $rejected = $false
        try { Resolve-BundleTarget $root $bad | Out-Null } catch { $rejected = $true }
        if (-not $rejected) { throw "Accepted traversal: $bad" }
    }
    $good = Resolve-BundleTarget $root 'data/libobs/shader.effect'
    if (-not $good.StartsWith($root)) { throw 'Valid file escaped staging' }
    $archive = Join-Path $root 'fake.zip'
    [System.IO.File]::WriteAllText($archive, 'tampered archive')
    $rejected = $false
    try { Assert-ArchiveHash $archive ('0' * 64) } catch { $rejected = $true }
    if (-not $rejected) { throw 'Accepted wrong hash' }
    Assert-ArchiveHash $archive (Get-Sha256 $archive)
    $rejected = $false
    try { Assert-BundleDestination $root | Out-Null } catch { $rejected = $true }
    if (-not $rejected) { throw 'Accepted unsafe replacement directory' }
    $cache = Join-Path $root 'obs'
    New-Item -ItemType Directory -Path $cache | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $cache 'VERSION.txt'), 'test')
    [System.IO.File]::WriteAllText((Join-Path $cache 'engine.dll'), 'original engine')
    [System.IO.File]::WriteAllText((Join-Path $cache 'mux.exe'), 'original muxer')
    $manifest = [ordered]@{}
    Get-ChildItem -LiteralPath $cache -File | ForEach-Object { $manifest[$_.Name] = Get-Sha256 $_.FullName }
    $manifest | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $cache 'SHA256SUMS.json') -Encoding UTF8
    if (-not (Test-BundleCache $cache 'test')) { throw 'Valid cache rejected' }
    [System.IO.File]::WriteAllText((Join-Path $cache 'engine.dll'), 'tampered engine')
    if (Test-BundleCache $cache 'test') { throw 'Tampered cache accepted' }
    $temporary = Join-Path $root ('obs.new-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $temporary | Out-Null
    Remove-BundleTemporaryDirectory $temporary $root 'obs.new-'
    if (Test-Path -LiteralPath $temporary) { throw 'Temporary directory was not removed' }
    $rejected = $false
    try { Remove-BundleTemporaryDirectory $cache $root 'obs.new-' } catch { $rejected = $true }
    if (-not $rejected -or -not (Test-Path -LiteralPath $cache)) { throw 'Cleanup accepted a non-temporary directory' }
    Write-Host 'PASS: 14 cases (paths, hashes, destination, cache, bounded cleanup)'
} finally {
    $resolved = [System.IO.Path]::GetFullPath($root)
    $tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if ($resolved.StartsWith($tempRoot) -and (Split-Path $resolved -Leaf).StartsWith('clipcat-zip-test-')) {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
