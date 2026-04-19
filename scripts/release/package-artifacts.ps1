param(
  [string]$WorkspaceRoot = (Get-Location).Path,
  [string]$OutputDir = $(Join-Path $env:RUNNER_TEMP 'prune-guard-artifacts')
)

$ErrorActionPreference = 'Stop'

# Package the release build into deterministic Windows artifacts.
# The script fails closed if the release output, installer inputs, or checksums are missing.

function Get-HostTriple {
  $hostLine = (& rustc -vV | Where-Object { $_ -like 'host: *' } | Select-Object -First 1)
  if (-not $hostLine) {
    throw 'Could not determine the Rust host triple.'
  }

  return $hostLine.Substring(6).Trim()
}

function Get-CargoPackageVersion {
  param(
    [string]$RootPath
  )

  $cargoTomlPath = Join-Path $RootPath 'Cargo.toml'
  if (-not (Test-Path -LiteralPath $cargoTomlPath)) {
    throw "Cargo.toml missing: $cargoTomlPath"
  }

  $inPackageSection = $false
  foreach ($line in (Get-Content -LiteralPath $cargoTomlPath)) {
    $trimmed = $line.Trim()

    if ($trimmed -match '^\[.+\]$') {
      $inPackageSection = $trimmed -eq '[package]'
      continue
    }

    if ($inPackageSection -and $trimmed -match '^version\s*=\s*"([^"]+)"\s*$') {
      return $Matches[1]
    }
  }

  throw "Could not find package.version in $cargoTomlPath"
}

function Get-DeterministicEpochUtc {
  # ZIP containers reject entry timestamps before 1980-01-01, so use the
  # earliest ZIP-safe instant for deterministic builds.
  return [DateTime]::SpecifyKind(
    [DateTime]::Parse('1980-01-01T00:00:00'),
    [DateTimeKind]::Utc
  )
}

function Resolve-InnoSetupCompiler {
  $command = Get-Command iscc -ErrorAction SilentlyContinue
  if ($command -and $command.Source) {
    return $command.Source
  }

  $candidates = @()
  if (${env:ProgramFiles(x86)}) {
    $candidates += (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe')
  }
  if ($env:ProgramFiles) {
    $candidates += (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe')
  }

  foreach ($candidate in $candidates) {
    if (Test-Path -LiteralPath $candidate) {
      return $candidate
    }
  }

  throw 'Inno Setup compiler (ISCC.exe) not found. Install Inno Setup 6 to build Windows installer artifacts.'
}

function Write-Sha256Manifest {
  param(
    [string]$ArtifactPath
  )

  if (-not (Test-Path -LiteralPath $ArtifactPath)) {
    throw "Artifact missing for checksum generation: $ArtifactPath"
  }

  $checksumPath = "$ArtifactPath.sha256"
  $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $ArtifactPath).Hash.ToLowerInvariant()
  Set-Content -LiteralPath $checksumPath -Value ("{0}  {1}" -f $hash, [System.IO.Path]::GetFileName($ArtifactPath))
  return $checksumPath
}

$hostTriple = Get-HostTriple
$appVersion = Get-CargoPackageVersion -RootPath $WorkspaceRoot
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

$daemonBinaryPath = Join-Path $releaseDir 'prune-guard.exe'
if (-not (Test-Path -LiteralPath $daemonBinaryPath)) {
  throw "Windows daemon binary missing: $daemonBinaryPath"
}
if ((Get-Item -LiteralPath $daemonBinaryPath).Length -le 0) {
  throw "Windows daemon binary is empty: $daemonBinaryPath"
}

$readmePath = Join-Path $WorkspaceRoot 'README.md'
if (-not (Test-Path -LiteralPath $readmePath)) {
  throw "README missing: $readmePath"
}

$installerScriptPath = Join-Path $WorkspaceRoot 'packaging/windows/prune-guard-installer.iss'
if (-not (Test-Path -LiteralPath $installerScriptPath)) {
  throw "Windows installer script missing: $installerScriptPath"
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

Add-Type -AssemblyName System.IO.Compression.FileSystem
Add-Type -AssemblyName System.IO.Compression

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

    $resolvedPackageRoot = (Resolve-Path -LiteralPath $packageRoot).Path
    if (-not $resolvedPackageRoot.EndsWith('\')) {
      $resolvedPackageRoot = "$resolvedPackageRoot\"
    }
    $packageRootUri = [Uri]$resolvedPackageRoot

    foreach ($file in $files) {
      # Use URI-based relative paths for compatibility with older
      # Windows PowerShell/.NET runtimes.
      $targetUri = [Uri]((Resolve-Path -LiteralPath $file.FullName).Path)
      $relativePath = [Uri]::UnescapeDataString($packageRootUri.MakeRelativeUri($targetUri).ToString()).Replace('\', '/')
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

$zipChecksumPath = Write-Sha256Manifest -ArtifactPath $archivePath

$installerBaseName = "prune-guard-$hostTriple-setup"
$installerPath = Join-Path $OutputDir ("$installerBaseName.exe")
if (Test-Path -LiteralPath $installerPath) {
  Remove-Item -LiteralPath $installerPath -Force
}

$isccPath = Resolve-InnoSetupCompiler
$isccArgs = @(
  "/DSourceBinary=$daemonBinaryPath"
  "/DSourceReadme=$readmePath"
  "/DInstallerOutputDir=$OutputDir"
  "/DInstallerBaseName=$installerBaseName"
  "/DAppVersion=$appVersion"
  $installerScriptPath
)

& $isccPath @isccArgs
if ($LASTEXITCODE -ne 0) {
  throw "ISCC failed with exit code $LASTEXITCODE"
}

if (-not (Test-Path -LiteralPath $installerPath)) {
  throw "Installer artifact missing after ISCC build: $installerPath"
}
if ((Get-Item -LiteralPath $installerPath).Length -le 0) {
  throw "Installer artifact is empty: $installerPath"
}

$installerChecksumPath = Write-Sha256Manifest -ArtifactPath $installerPath

Write-Output $archivePath
Write-Output $zipChecksumPath
Write-Output $installerPath
Write-Output $installerChecksumPath
