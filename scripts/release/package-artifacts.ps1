param(
  [string]$WorkspaceRoot = (Get-Location).Path,
  [string]$OutputDir = $(Join-Path $env:RUNNER_TEMP 'prune-guard-artifacts')
)

$ErrorActionPreference = 'Stop'

# Package the release build into a deterministic zip archive.
# The script fails closed if the release output is missing or empty.

function Get-HostTriple {
  $hostLine = (& rustc -vV | Where-Object { $_ -like 'host: *' } | Select-Object -First 1)
  if (-not $hostLine) {
    throw 'Could not determine the Rust host triple.'
  }

  return $hostLine.Substring(6).Trim()
}

function Get-DeterministicEpochUtc {
  # Avoid relying on UnixEpoch static properties, which are inconsistent across
  # some Windows/.NET combinations used in CI images.
  return [DateTime]::SpecifyKind(
    [DateTime]::Parse('1970-01-01T00:00:00'),
    [DateTimeKind]::Utc
  )
}

$hostTriple = Get-HostTriple
$releaseDir = Join-Path $WorkspaceRoot 'target/release'

if (-not (Test-Path -LiteralPath $releaseDir)) {
  throw "Release directory missing: $releaseDir"
}

$nonEmptyFile = Get-ChildItem -LiteralPath $releaseDir -Recurse -File |
  Where-Object { $_.Length -gt 0 } |
  Select-Object -First 1

if (-not $nonEmptyFile) {
  throw 'Release directory does not contain any non-empty files.'
}

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

$stagingRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("prune-guard-package-{0}" -f [System.Guid]::NewGuid().ToString('N'))
$packageName = "prune-guard-$hostTriple"
$packageRoot = Join-Path $stagingRoot $packageName
$metadataRoot = Join-Path $packageRoot 'metadata'
$releaseCopyRoot = Join-Path $packageRoot 'release'

New-Item -ItemType Directory -Force -Path $metadataRoot | Out-Null
New-Item -ItemType Directory -Force -Path $releaseCopyRoot | Out-Null

Copy-Item -Path (Join-Path $releaseDir '*') -Destination $releaseCopyRoot -Recurse -Force
Copy-Item -LiteralPath (Join-Path $WorkspaceRoot 'Cargo.toml') -Destination (Join-Path $metadataRoot 'Cargo.toml')
Copy-Item -LiteralPath (Join-Path $WorkspaceRoot 'Cargo.lock') -Destination (Join-Path $metadataRoot 'Cargo.lock')
Copy-Item -LiteralPath (Join-Path $WorkspaceRoot 'README.md') -Destination (Join-Path $metadataRoot 'README.md')

# Normalise timestamps so the archive is reproducible for the same inputs.
$epochUtc = Get-DeterministicEpochUtc
$epochOffset = [DateTimeOffset]::new($epochUtc)
Get-ChildItem -LiteralPath $packageRoot -Recurse -Force | ForEach-Object {
  try {
    $_.LastWriteTimeUtc = $epochUtc
  }
  catch {
    throw "Failed to set deterministic timestamp on '$($_.FullName)': $($_.Exception.Message)"
  }
}

$archivePath = Join-Path $OutputDir ("$packageName.zip")
$checksumPath = "$archivePath.sha256"

Add-Type -AssemblyName System.IO.Compression.FileSystem

if (Test-Path -LiteralPath $archivePath) {
  Remove-Item -LiteralPath $archivePath -Force
}

$fileStream = [System.IO.File]::Open($archivePath, [System.IO.FileMode]::CreateNew)
try {
$zip = New-Object -TypeName System.IO.Compression.ZipArchive -ArgumentList @(
    $fileStream,
    [System.IO.Compression.ZipArchiveMode]::Create,
    $false,
    [System.Text.Encoding]::UTF8
  )

  try {
    $files = Get-ChildItem -LiteralPath $packageRoot -Recurse -File | Sort-Object FullName

    if (-not $files) {
      throw 'Package staging area contains no files.'
    }

    foreach ($file in $files) {
      $relativePath = [System.IO.Path]::GetRelativePath($packageRoot, $file.FullName).Replace('\', '/')
      $entry = $zip.CreateEntry($relativePath, [System.IO.Compression.CompressionLevel]::Optimal)
      $entry.LastWriteTime = $epochOffset

      $entryStream = $entry.Open()
      try {
        $sourceStream = [System.IO.File]::OpenRead($file.FullName)
        try {
          $sourceStream.CopyTo($entryStream)
        }
        finally {
          $sourceStream.Dispose()
        }
      }
      finally {
        $entryStream.Dispose()
      }
    }
  }
  finally {
    $zip.Dispose()
  }
}
finally {
  $fileStream.Dispose()
}

$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash.ToLowerInvariant()
Set-Content -LiteralPath $checksumPath -Value ("{0}  {1}" -f $hash, [System.IO.Path]::GetFileName($archivePath))

Write-Output $archivePath
Write-Output $checksumPath
