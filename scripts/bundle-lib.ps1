function Get-Sha256([string]$Path) {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    $stream = [System.IO.File]::OpenRead($Path)
    try { return -join ($sha.ComputeHash($stream) | ForEach-Object { $_.ToString('x2') }) }
    finally { $stream.Dispose(); $sha.Dispose() }
}

function Assert-ArchiveHash([string]$Path, [string]$Expected) {
    $actual = Get-Sha256 $Path
    if ($actual -ne $Expected) { throw "Archive SHA256 mismatch: $actual" }
}

function Resolve-BundleTarget([string]$Root, [string]$Relative) {
    # ZIP member names are untrusted, including entries matched by a wildcard.
    $parts = $Relative.Replace('\', '/').Split('/')
    if ([System.IO.Path]::IsPathRooted($Relative) -or $Relative.Contains(':') -or
        $parts -contains '..' -or $parts -contains '.' -or [string]::IsNullOrWhiteSpace($Relative)) {
        throw "Unsafe archive path: $Relative"
    }
    $base = [System.IO.Path]::GetFullPath($Root).TrimEnd('\', '/') + [System.IO.Path]::DirectorySeparatorChar
    $target = [System.IO.Path]::GetFullPath((Join-Path $base $Relative))
    if (-not $target.StartsWith($base, [System.StringComparison]::OrdinalIgnoreCase)) { throw "Archive path escapes staging: $Relative" }
    return $target
}

function Assert-BundleDestination([string]$Path) {
    $full = [System.IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
    if ([System.IO.Path]::GetFileName($full) -ne 'obs') { throw 'Bundle destination must be a dedicated directory named obs.' }
    if (Test-Path -LiteralPath $full) {
        $entry = Get-Item -LiteralPath $full -Force
        if ($entry.Attributes -band [System.IO.FileAttributes]::ReparsePoint) { throw 'Bundle destination cannot be a link.' }
        if (-not (Test-Path -LiteralPath (Join-Path $full 'VERSION.txt')) -and
            @(Get-ChildItem -LiteralPath $full -Force).Count -gt 0) { throw 'Refusing to replace a directory without ClipCat bundle metadata.' }
    }
    return $full
}

function Test-BundleCache([string]$Root, [string]$Tag) {
    try {
        $versionFile = Join-Path $Root 'VERSION.txt'
        if ((Get-Content -LiteralPath $versionFile -Raw).Trim() -ne $Tag) { return $false }
        $manifest = Get-Content -LiteralPath (Join-Path $Root 'SHA256SUMS.json') -Raw | ConvertFrom-Json
        if (@($manifest.PSObject.Properties).Count -lt 3) { return $false }
        foreach ($file in $manifest.PSObject.Properties) {
            $path = Resolve-BundleTarget $Root $file.Name
            if ((Get-Sha256 $path) -ne $file.Value) { return $false }
        }
        $files = @(Get-ChildItem -LiteralPath $Root -Recurse -File | Where-Object { $_.Name -ne 'SHA256SUMS.json' })
        return $files.Count -eq @($manifest.PSObject.Properties).Count
    } catch { return $false }
}

function Remove-BundleTemporaryDirectory([string]$Path, [string]$Parent, [string]$Prefix) {
    $full = [System.IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
    $expectedParent = [System.IO.Path]::GetFullPath($Parent).TrimEnd('\', '/')
    $leaf = [System.IO.Path]::GetFileName($full)
    if ([System.IO.Path]::GetDirectoryName($full) -ne $expectedParent -or
        $leaf -notmatch ('^' + [regex]::Escape($Prefix) + '[0-9a-f]{32}$')) {
        throw 'Temporary bundle cleanup path failed validation.'
    }
    if (Test-Path -LiteralPath $full) {
        if ((Get-Item -LiteralPath $full -Force).Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
            throw 'Temporary bundle cleanup refuses links.'
        }
        Remove-Item -LiteralPath $full -Recurse -Force
    }
}
