# Shared Windows adapter for foreign build hosts.
# Jet owns checking and Library export. This script owns identity, bounded
# locking, staging, and the final host artifact-set commit marker.

[CmdletBinding()]
param(
    [string]$Jet = "jet",
    [Parameter(Mandatory = $true)][string]$Project,
    [Parameter(Mandatory = $true)][string]$Entry,
    [Parameter(Mandatory = $true)][string]$Output,
    [Parameter(Mandatory = $true)][string]$Library,
    [Parameter(Mandatory = $true)][string]$Dest,
    [string]$Kind = "static",
    [string]$Profile = "dev",
    [string]$Toolchain = "unknown",
    [switch]$Loadable,
    [switch]$StageProject,
    [ValidateRange(1, 86400)][int]$TimeoutSeconds = 900,
    [string[]]$InputPath = @()
)

$ErrorActionPreference = "Stop"
$publishComplete = $false
$lockOwned = $false
$lockDir = $null
$stage = $null
$stageProjectDir = $null
$runProject = $null
$jetProcess = $null
# PowerShell rejects repeated named array parameters. `|` cannot occur in a
# Windows filename, so host adapters pass the complete input closure once.
$inputPaths = @(
    foreach ($value in @($InputPath)) {
        foreach ($path in ([string]$value -split '\|')) {
            if (-not [String]::IsNullOrEmpty($path)) { $path }
        }
    }
)

function Fail([string]$Message) {
    throw "jet-host: $Message"
}

function Test-Reparse([string]$Path) {
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
    if ($null -eq $item) {
        return $false
    }
    return (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)
}

function Assert-NoReparseComponents([string]$Path, [string]$Label) {
    $current = [IO.Path]::GetFullPath($Path)
    while ($true) {
        if (Test-Reparse $current) {
            Fail "JET-HOST-INPUT: $Label contains a symlink or reparse point at '$current'"
        }
        $parent = Split-Path -Parent $current
        if ([String]::IsNullOrEmpty($parent) -or $parent -eq $current) {
            break
        }
        $current = $parent
    }
}

function Get-RealDirectory([string]$Path, [string]$Label) {
    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        Fail "JET-HOST-INPUT: $Label '$Path' is not a real directory"
    }
    Assert-NoReparseComponents $Path $Label
    $first = [IO.Path]::GetFullPath((Get-Item -LiteralPath $Path -Force).FullName)
    $second = [IO.Path]::GetFullPath((Get-Item -LiteralPath $Path -Force).FullName)
    if ($first -cne $second) {
        Fail "JET-HOST-INPUT: $Label '$Path' changed while it was being resolved"
    }
    return $second
}

function Resolve-Executable([string]$Name, [string]$Label) {
    $command = Get-Command -Name $Name -ErrorAction SilentlyContinue
    if ($null -eq $command) {
        Fail "JET-HOST-TOOL: $Label executable '$Name' is not available"
    }
    $path = $command.Path
    if ([String]::IsNullOrEmpty($path)) {
        $path = $command.Source
    }
    if ([String]::IsNullOrEmpty($path)) {
        Fail "JET-HOST-TOOL: $Label command '$Name' has no executable path"
    }
    return [IO.Path]::GetFullPath($path)
}

function Get-ProjectFile([string]$Root, [string]$Raw, [string]$Label) {
    $candidate = if ([IO.Path]::IsPathRooted($Raw)) {
        [IO.Path]::GetFullPath($Raw)
    } else {
        [IO.Path]::GetFullPath((Join-Path $Root $Raw))
    }
    $prefix = $Root.TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    if (-not $candidate.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        Fail "JET-HOST-INPUT: '$Raw' escapes the Jet project root"
    }
    if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) {
        Fail "JET-HOST-INPUT: missing '$Raw' under '$Root'"
    }
    Assert-NoReparseComponents $candidate $Label
    if (Test-Reparse $candidate) {
        Fail "JET-HOST-INPUT: '$Raw' is a symlink or reparse point"
    }
    return $candidate
}

function Get-JetSourceClosure([string]$Program, [string]$Root, [string]$Destination) {
    $listing = New-IdentityFile $Destination
    try {
        Push-Location -LiteralPath $Root
        try {
            & $Program project parts *> $listing
            if ($LASTEXITCODE -ne 0) {
                Fail "JET-HOST-INPUT: Jet project parts could not derive the source closure"
            }
        } finally {
            Pop-Location
        }
        $relativePaths = @()
        $seen = @{}
        foreach ($line in @(Get-Content -LiteralPath $listing -ErrorAction Stop)) {
            $fields = ([string]$line).Trim() -split '\s+', 3
            if ($fields.Count -ne 3) {
                continue
            }
            $relative = $fields[2].Trim()
            if (-not $relative.EndsWith('.jet', [StringComparison]::OrdinalIgnoreCase)) {
                continue
            }
            $full = Get-ProjectFile $Root $relative "source"
            $relative = $full.Substring($Root.Length + 1)
            if (-not $seen.ContainsKey($relative)) {
                $seen[$relative] = $true
                $relativePaths += $relative
            }
        }
        if ($relativePaths.Count -eq 0) {
            Fail "JET-HOST-INPUT: Jet project parts returned an empty source closure"
        }
        return $relativePaths
    } finally {
        Remove-OwnedFile $listing
    }
}

function New-IdentityFile([string]$Root) {
    Assert-NoReparseComponents $Root "destination"
    if (-not (Test-Path -LiteralPath $Root -PathType Container) -or (Test-Reparse $Root)) {
        Fail "JET-HOST-OUTPUT: destination root changed"
    }
    do {
        $path = Join-Path $Root (".jet-host-identity-" + [Guid]::NewGuid().ToString("N"))
    } while (Test-Path -LiteralPath $path)
    New-Item -ItemType File -Path $path -Force:$false | Out-Null
    return $path
}

function Get-Sha256([string]$Path) {
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
    if ($null -eq $item -or $item.PSIsContainer -or (Test-Reparse $Path)) {
        Fail "JET-HOST-INPUT: cannot hash non-regular file '$Path'"
    }
    return "sha256-" + (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Invoke-VersionIdentity([string]$Program, [string[]]$Arguments, [string]$Label, [string]$Root) {
    $file = New-IdentityFile $Root
    try {
        & $Program @Arguments *> $file
        if ($LASTEXITCODE -ne 0) {
            Fail "JET-HOST-TOOL: could not query $Label identity"
        }
        $version = (Get-Content -LiteralPath $file -TotalCount 1 -ErrorAction SilentlyContinue)
        $output = Get-Content -LiteralPath $file -Raw -ErrorAction Stop
        [pscustomobject]@{
            path = $Program
            version = if ($null -eq $version) { "" } else { [string]$version }
            output = $output
            identity = Get-Sha256 $file
        }
    } finally {
        Remove-Item -LiteralPath $file -Force -ErrorAction SilentlyContinue
    }
}

function Remove-OwnedFile([string]$Path) {
    Assert-NoReparseComponents $Path "owned path"
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
    if ($null -eq $item) {
        return
    }
    if (Test-Reparse $Path) {
        Fail "JET-HOST-INPUT: refusing to remove symlink or reparse point '$Path'"
    }
    if ($item.PSIsContainer) {
        Fail "JET-HOST-INPUT: refusing to remove directory where a file belongs: '$Path'"
    }
    Remove-Item -LiteralPath $Path -Force
}

function Remove-OwnedDirectory([string]$Path) {
    Assert-NoReparseComponents $Path "owned path"
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
    if ($null -eq $item) {
        return
    }
    if (Test-Reparse $Path) {
        Fail "JET-HOST-INPUT: refusing to remove symlink or reparse point '$Path'"
    }
    if (-not $item.PSIsContainer) {
        Fail "JET-HOST-INPUT: refusing to remove a file where a directory belongs: '$Path'"
    }
    Remove-Item -LiteralPath $Path -Recurse -Force
}

function Assert-PublishPath([string]$Path) {
    Assert-NoReparseComponents $Path "publication path"
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
    if ($null -eq $item) {
        return
    }
    if (Test-Reparse $Path) {
        Fail "JET-HOST-INPUT: refusing to replace symlink or reparse point '$Path'"
    }
    if ($item.PSIsContainer) {
        Fail "JET-HOST-INPUT: refusing to replace directory '$Path'"
    }
}

function Quote-WindowsArgument([string]$Value) {
    $builder = New-Object Text.StringBuilder
    [void]$builder.Append('"')
    $slashes = 0
    foreach ($character in $Value.ToCharArray()) {
        if ($character -eq [char]92) {
            $slashes++
            continue
        }
        if ($character -eq [char]34) {
            for ($index = 0; $index -lt (2 * $slashes + 1); $index++) {
                [void]$builder.Append([char]92)
            }
            [void]$builder.Append([char]34)
            $slashes = 0
            continue
        }
        for ($index = 0; $index -lt $slashes; $index++) {
            [void]$builder.Append([char]92)
        }
        $slashes = 0
        [void]$builder.Append($character)
    }
    for ($index = 0; $index -lt (2 * $slashes); $index++) {
        [void]$builder.Append([char]92)
    }
    [void]$builder.Append('"')
    return $builder.ToString()
}

function Stop-JetProcessTree([int]$ProcessId) {
    if ($ProcessId -le 0) {
        return
    }
    # taskkill is part of Windows and includes rustc/linker descendants. The
    # Stop-Process fallback still handles restricted shells and keeps cleanup
    # deterministic when taskkill cannot inspect the process.
    & taskkill.exe /PID $ProcessId /T /F *> $null
    if ($LASTEXITCODE -ne 0) {
        Stop-Process -Id $ProcessId -Force -ErrorAction SilentlyContinue
    }
}

function Invoke-JetBuild(
    [string]$Program,
    [string[]]$Arguments,
    [string]$WorkingDirectory,
    [int]$TimeoutSeconds,
    [string]$Rustc,
    [string]$Linker
) {
    $startInfo = New-Object Diagnostics.ProcessStartInfo
    $startInfo.FileName = $Program
    $startInfo.Arguments = (($Arguments | ForEach-Object {
        Quote-WindowsArgument ([string]$_)
    }) -join " ")
    $startInfo.WorkingDirectory = $WorkingDirectory
    $startInfo.UseShellExecute = $false
    $startInfo.EnvironmentVariables["RUSTC"] = $Rustc
    $startInfo.EnvironmentVariables["RUSTC_LINKER"] = $Linker
    $startInfo.EnvironmentVariables["CC"] = $Linker
    $startInfo.EnvironmentVariables["NO_COLOR"] = "1"
    $rustcDirectory = Split-Path -Parent $Rustc
    $startInfo.EnvironmentVariables["PATH"] = "$rustcDirectory$([IO.Path]::PathSeparator)$($env:PATH)"
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $startInfo
    $started = $false
    try {
        if (-not $process.Start()) {
            Fail "JET-HOST-BUILD: could not start Jet"
        }
        $started = $true
        $script:jetProcess = $process
        $buildDeadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
        while (-not $process.HasExited) {
            if ([DateTime]::UtcNow -ge $buildDeadline) {
                Stop-JetProcessTree $process.Id
                $process.WaitForExit(5000) | Out-Null
                Fail "JET-HOST-BUILD: timed out after ${TimeoutSeconds}s; no new host artifact was published"
            }
            Start-Sleep -Milliseconds 100
        }
        return $process.ExitCode
    } finally {
        if ($started -and -not $process.HasExited) {
            Stop-JetProcessTree $process.Id
            $process.WaitForExit()
        }
        if ($script:jetProcess -eq $process) {
            $script:jetProcess = $null
        }
        $process.Dispose()
    }
}

function Published-Names([string]$Name) {
    @(
        "jet-host.stamp",
        "jet-host.receipt",
        ("lib{0}.a" -f $Name),
        ("lib{0}.so" -f $Name),
        ("lib{0}.dylib" -f $Name),
        ("lib{0}.dll" -f $Name),
        ("{0}.h" -f $Name),
        ("{0}.jetlib" -f $Name)
    )
}

function Clean-Destination([string]$Root, [string]$Name) {
    Assert-NoReparseComponents $Root "destination"
    $rootItem = Get-Item -LiteralPath $Root -Force -ErrorAction SilentlyContinue
    if ($null -eq $rootItem -or -not $rootItem.PSIsContainer -or (Test-Reparse $Root)) {
        Fail "JET-HOST-OUTPUT: destination root changed"
    }
    $paths = @(Published-Names $Name | ForEach-Object { Join-Path $Root $_ })
    foreach ($path in $paths) {
        Assert-PublishPath $path
    }
    foreach ($path in $paths) {
        Remove-OwnedFile $path
    }
}

try {
    if ($Kind -notin @("static", "shared", "both")) {
        Fail "JET-HOST-ABI: unsupported Library kind '$Kind'"
    }
    if ($Library -notmatch "^[A-Za-z0-9_-]+$") {
        Fail "JET-HOST-ABI: Library name '$Library' is not a stable native artifact name"
    }
    if ($Output -notmatch "^[A-Za-z0-9_-]+$") {
        Fail "JET-HOST-INPUT: output name '$Output' is not a manifest output name"
    }
    if ($Profile -notmatch "^[A-Za-z0-9_.-]*$") {
        Fail "JET-HOST-INPUT: profile '$Profile' is not a stable profile name"
    }
    if ($TimeoutSeconds -lt 1 -or $TimeoutSeconds -gt 86400) {
        Fail "JET-HOST-INPUT: timeout must be a whole number from 1 through 86400 seconds"
    }

    $jetCommand = Resolve-Executable $Jet "Jet"
    $projectItem = Get-Item -LiteralPath $Project -Force -ErrorAction SilentlyContinue
    if ($null -eq $projectItem -or -not $projectItem.PSIsContainer) {
        Fail "JET-HOST-INPUT: project directory '$Project' does not exist"
    }
    $projectFull = Get-RealDirectory $Project "project"
    Assert-NoReparseComponents $projectFull "project"

    $destCandidate = [IO.Path]::GetFullPath($Dest)
    $destItem = Get-Item -LiteralPath $destCandidate -Force -ErrorAction SilentlyContinue
    if ($null -eq $destItem) {
        Assert-NoReparseComponents (Split-Path -Parent $destCandidate) "destination parent"
        New-Item -ItemType Directory -Path $destCandidate -Force | Out-Null
    } elseif (-not $destItem.PSIsContainer) {
        Fail "JET-HOST-INPUT: destination '$Dest' is not a directory"
    }
    $destFull = Get-RealDirectory $destCandidate "destination"

    $entryFull = Get-ProjectFile $projectFull $Entry "entry"
    $entryRel = $entryFull.Substring($projectFull.Length + 1)
    $sourceClosure = @(Get-JetSourceClosure $jetCommand $projectFull $destFull)
    $rawInputs = @("package.jet", ".jet\lock", $Entry) + $sourceClosure + $inputPaths
    $inputFiles = @()
    $inputRels = @()
    $inputDigests = @()
    $seen = @{}
    foreach ($raw in $rawInputs) {
        $full = Get-ProjectFile $projectFull $raw "input"
        $rel = $full.Substring($projectFull.Length + 1)
        if (-not $seen.ContainsKey($rel)) {
            $seen[$rel] = $true
            $inputFiles += $full
            $inputRels += $rel
            $inputDigests += Get-Sha256 $full
        }
    }
    $lockDigest = $inputDigests[1]

    $jetDir = Join-Path $projectFull ".jet"
    if (-not (Test-Path -LiteralPath $jetDir)) {
        New-Item -ItemType Directory -Path $jetDir -Force | Out-Null
    }
    if (Test-Reparse $jetDir) {
        Fail "JET-HOST-INPUT: .jet is a symlink or reparse point"
    }
    $lockDir = Join-Path $jetDir "foreign-host.lock"
    $lockItem = Get-Item -LiteralPath $lockDir -Force -ErrorAction SilentlyContinue
    if ($null -ne $lockItem) {
        if (-not $lockItem.PSIsContainer -or (Test-Reparse $lockDir)) {
            Fail "JET-HOST-TOOL: lock path '$lockDir' is not a real directory"
        }
    }

    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while ($true) {
        try {
            New-Item -ItemType Directory -Path $lockDir -ErrorAction Stop | Out-Null
            $lockOwned = $true
            $lockCheck = Get-Item -LiteralPath $lockDir -Force -ErrorAction SilentlyContinue
            if ($null -eq $lockCheck -or -not $lockCheck.PSIsContainer -or (Test-Reparse $lockDir)) {
                Fail "JET-HOST-TOOL: lock path '$lockDir' changed during acquisition"
            }
            break
        } catch {
            if ([DateTime]::UtcNow -ge $deadline) {
                Fail "JET-HOST-TOOL: timed out waiting for '$lockDir'"
            }
            $ownerPath = Join-Path $lockDir "pid"
            $ownerText = if (Test-Path -LiteralPath $ownerPath) { (Get-Content -LiteralPath $ownerPath -Raw).Trim() } else { "" }
            [int]$ownerId = 0
            if ([int]::TryParse($ownerText, [ref]$ownerId)) {
                $owner = Get-Process -Id $ownerId -ErrorAction SilentlyContinue
                if ($null -eq $owner) {
                    Remove-OwnedFile $ownerPath
                    Remove-OwnedFile (Join-Path $lockDir "started")
                    Remove-Item -LiteralPath $lockDir -Force -ErrorAction SilentlyContinue
                    continue
                }
            }
            Start-Sleep -Milliseconds 200
        }
    }
    Set-Content -LiteralPath (Join-Path $lockDir "pid") -Value ([string]$PID) -Encoding ASCII
    Set-Content -LiteralPath (Join-Path $lockDir "started") -Value ([DateTime]::UtcNow.ToString("O")) -Encoding ASCII

    $runProject = $projectFull
    if ($StageProject) {
        do {
            $stageProjectDir = Join-Path $destFull (".jet-project-" + [Guid]::NewGuid().ToString("N"))
        } while (Test-Path -LiteralPath $stageProjectDir)
        New-Item -ItemType Directory -Path $stageProjectDir | Out-Null
        for ($index = 0; $index -lt $inputFiles.Count; $index++) {
            $relative = $inputRels[$index] -replace '\\', '/'
            $relativeDestination = Join-Path $stageProjectDir ($relative -replace '/', [string][IO.Path]::DirectorySeparatorChar)
            $relativeParent = Split-Path -Parent $relativeDestination
            if (-not (Test-Path -LiteralPath $relativeParent -PathType Container)) {
                New-Item -ItemType Directory -Path $relativeParent -Force | Out-Null
            }
            Copy-Item -LiteralPath $inputFiles[$index] -Destination $relativeDestination
        }
        Remove-OwnedDirectory (Join-Path $stageProjectDir "target")
        Remove-OwnedDirectory (Join-Path $stageProjectDir ".jet\foreign-host.lock")
        $runProject = Get-RealDirectory $stageProjectDir "staged project"
    }

    $jetInfo = Invoke-VersionIdentity $jetCommand @("--version") "Jet" $destFull
    $rustcName = if ([String]::IsNullOrWhiteSpace($env:RUSTC)) { "rustc" } else { $env:RUSTC }
    $rustcCommand = Resolve-Executable $rustcName "rustc"
    $rustcFile = New-IdentityFile $destFull
    try {
        & $rustcCommand -vV *> $rustcFile
        if ($LASTEXITCODE -ne 0) {
            Fail "JET-HOST-TOOL: could not query rustc target identity"
        }
        $rustcIdentity = Get-Sha256 $rustcFile
        $rustcText = Get-Content -LiteralPath $rustcFile -Raw
    } finally {
        Remove-Item -LiteralPath $rustcFile -Force -ErrorAction SilentlyContinue
    }
    $targetMatch = [regex]::Match($rustcText, "(?m)^host:\s*(.+)$")
    if (-not $targetMatch.Success) {
        Fail "JET-HOST-TOOL: rustc did not report a host target"
    }
    $targetTriple = $targetMatch.Groups[1].Value.Trim()
    $rustcVersion = (($rustcText -split "`r?`n", 2)[0]).Trim()
    $sysrootInfo = Invoke-VersionIdentity $rustcCommand @("--print", "sysroot") "rustc sysroot" $destFull
    $sysrootPath = $sysrootInfo.output.Trim()
    if ([String]::IsNullOrEmpty($sysrootPath)) {
        Fail "JET-HOST-TOOL: rustc did not report a sysroot"
    }
    $targetLibdirInfo = Invoke-VersionIdentity $rustcCommand @("--print", "target-libdir") "rustc target library directory" $destFull
    $targetLibdir = $targetLibdirInfo.output.Trim()
    if ([String]::IsNullOrEmpty($targetLibdir)) {
        Fail "JET-HOST-TOOL: rustc did not report a target library directory"
    }

    $toolchainName = $Toolchain
    $toolchainPath = "unspecified"
    $toolchainVersion = "not-specified"
    $toolchainIdentity = "not-specified"
    if (-not [String]::IsNullOrWhiteSpace($Toolchain) -and $Toolchain -ne "unknown") {
        $toolchainPath = Resolve-Executable $Toolchain "host toolchain"
        $toolchainInfo = Invoke-VersionIdentity $toolchainPath @("--version") "host toolchain" $destFull
        $toolchainVersion = $toolchainInfo.version
        $toolchainIdentity = $toolchainInfo.identity
    }
    $linkerName = $toolchainName
    $linkerPath = $toolchainPath
    $linkerVersion = $toolchainVersion
    $linkerIdentity = $toolchainIdentity
    if ([String]::IsNullOrWhiteSpace($Toolchain) -or $Toolchain -eq "unknown") {
        $linkerName = if (-not [String]::IsNullOrWhiteSpace($env:RUSTC_LINKER)) {
            $env:RUSTC_LINKER
        } elseif (-not [String]::IsNullOrWhiteSpace($env:CC)) {
            $env:CC
        } else {
            # rustc has no stable `--print linker` query. Match its normal
            # host selection and let Resolve-Executable report a missing cc.
            "cc"
        }
        if ([String]::IsNullOrWhiteSpace($linkerName)) {
            Fail "JET-HOST-TOOL: no linker was selected"
        }
        $linkerPath = Resolve-Executable $linkerName "linker"
        $linkerInfo = Invoke-VersionIdentity $linkerPath @("--version") "linker" $destFull
        $linkerVersion = $linkerInfo.version
        $linkerIdentity = $linkerInfo.identity
    }
    $linkerLeaf = [IO.Path]::GetFileName($linkerPath).ToLowerInvariant()
    if ($linkerLeaf -in @("cl", "cl.exe", "link", "link.exe")) {
        Fail "JET-HOST-ABI: Jet emits GNU .a artifacts; use a GNU-compatible host toolchain instead of MSVC"
    }

    Clean-Destination $destFull $Library

    $jetArgs = @("build", "--lib", "--locked", "--output", $Output)
    if (-not [String]::IsNullOrEmpty($Profile)) {
        $jetArgs += "--profile=$Profile"
    }
    $jetArgs += $entryRel
    $jetStatus = Invoke-JetBuild $jetCommand $jetArgs $runProject $TimeoutSeconds $rustcCommand $linkerPath
    if ($jetStatus -ne 0) {
        Fail "JET-HOST-BUILD: Jet Library build failed with status $jetStatus; no new host artifact was published"
    }

    $target = Get-RealDirectory (Join-Path $runProject "target") "Jet target"
    $artifacts = @()
    if ($Kind -eq "static" -or $Kind -eq "both") { $artifacts += ("lib{0}.a" -f $Library) }
    if ($Kind -eq "shared" -or $Kind -eq "both") { $artifacts += ("lib{0}.dll" -f $Library) }
    $artifacts += ("{0}.h" -f $Library)
    if ($Loadable) { $artifacts += ("{0}.jetlib" -f $Library) }
    $knownArtifacts = @(
        "lib$Library.a",
        "lib$Library.so",
        "lib$Library.dylib",
        "lib$Library.dll",
        "$Library.h",
        "$Library.jetlib",
        "bindings/$Library.h",
        "bindings/$Library.py",
        "bindings/$Library.swift"
    )

    foreach ($artifact in $artifacts) {
        $source = Join-Path $target $artifact
        if (-not (Test-Path -LiteralPath $source -PathType Leaf) -or (Test-Reparse $source)) {
            Fail "JET-HOST-ABI: Jet did not produce target/$artifact; check the Library output kind and host ABI"
        }
    }
    $completion = Join-Path $target (".{0}.jet-library.complete" -f $Library)
    if (-not (Test-Path -LiteralPath $completion -PathType Leaf) -or (Test-Reparse $completion)) {
        Fail "JET-HOST-ABI: Jet did not publish a complete Library artifact set"
    }
    $completionLines = @(Get-Content -LiteralPath $completion -ErrorAction Stop)
    if ($completionLines.Count -eq 0 -or $completionLines[0] -cne "jet-library-set-v1") {
        Fail "JET-HOST-ABI: Jet Library completion marker is invalid"
    }
    $markerRecords = @{}
    foreach ($line in ($completionLines | Select-Object -Skip 1)) {
        $parts = ([string]$line).Split([char]9)
        if ($parts.Count -ne 2 -or [String]::IsNullOrEmpty($parts[0]) -or
            $parts[1] -notmatch "^sha256-[0-9a-fA-F]{64}$") {
            Fail "JET-HOST-ABI: completion marker contains an invalid entry"
        }
        $markerName = $parts[0] -replace '\\', '/'
        if ([IO.Path]::IsPathRooted($parts[0]) -or $parts[0] -match '^[A-Za-z]:') {
            Fail "JET-HOST-ABI: completion marker contains an unsafe artifact path '$($parts[0])'"
        }
        $markerParts = $markerName.Split('/')
        if ($markerParts | Where-Object { [String]::IsNullOrEmpty($_) -or $_ -eq '.' -or $_ -eq '..' }) {
            Fail "JET-HOST-ABI: completion marker contains an unsafe artifact path '$($parts[0])'"
        }
        if (-not ($knownArtifacts -contains $markerName)) {
            Fail "JET-HOST-ABI: completion marker names an unexpected artifact '$markerName'"
        }
        if ($markerRecords.ContainsKey($markerName)) {
            Fail "JET-HOST-ABI: completion marker repeats target/$markerName"
        }
        $markerRecords[$markerName] = $parts[1]
    }
    if ($markerRecords.Count -eq 0) {
        Fail "JET-HOST-ABI: completion marker does not describe any artifact"
    }
    foreach ($markerName in @($markerRecords.Keys)) {
        $targetPath = Join-Path $target ($markerName -replace '/', [string][IO.Path]::DirectorySeparatorChar)
        $targetItem = Get-Item -LiteralPath $targetPath -Force -ErrorAction SilentlyContinue
        if ($null -eq $targetItem -or $targetItem.PSIsContainer -or $targetItem.Length -le 0 -or (Test-Reparse $targetPath)) {
            Fail "JET-HOST-ABI: completion marker names missing target/$markerName"
        }
        if ($markerRecords[$markerName] -cne (Get-Sha256 $targetPath)) {
            Fail "JET-HOST-ABI: completion marker does not match target/$markerName"
        }
    }
    foreach ($artifact in $artifacts) {
        if (-not $markerRecords.ContainsKey($artifact)) {
            Fail "JET-HOST-ABI: completion marker omits target/$artifact"
        }
    }
    foreach ($knownArtifact in $knownArtifacts) {
        $knownPath = Join-Path $target ($knownArtifact -replace '/', [string][IO.Path]::DirectorySeparatorChar)
        if (Test-Path -LiteralPath $knownPath) {
            if (-not $markerRecords.ContainsKey($knownArtifact)) {
                Fail "JET-HOST-ABI: target contains stale uncommitted artifact '$knownArtifact'"
            }
        }
    }
    if (-not $StageProject) {
        for ($index = 0; $index -lt $inputFiles.Count; $index++) {
            if ((Get-Sha256 $inputFiles[$index]) -cne $inputDigests[$index]) {
                Fail "JET-HOST-INPUT: input '$($inputRels[$index])' changed during the Jet build"
            }
        }
    }

    do {
        $stage = Join-Path $destFull (".jet-host-stage-" + [Guid]::NewGuid().ToString("N"))
    } while (Test-Path -LiteralPath $stage)
    Assert-NoReparseComponents $destFull "destination"
    if (-not (Test-Path -LiteralPath $destFull -PathType Container) -or (Test-Reparse $destFull)) {
        Fail "JET-HOST-OUTPUT: destination root changed"
    }
    New-Item -ItemType Directory -Path $stage | Out-Null

    $artifactRecords = @()
    foreach ($artifact in $artifacts) {
        Copy-Item -LiteralPath (Join-Path $target $artifact) -Destination (Join-Path $stage $artifact)
        $copy = Join-Path $stage $artifact
        $artifactRecords += [ordered]@{
            name = $artifact
            bytes = (Get-Item -LiteralPath $copy -Force).Length
            digest = Get-Sha256 $copy
        }
    }
    $inputRecords = @()
    for ($index = 0; $index -lt $inputRels.Count; $index++) {
        $inputRecords += [ordered]@{
            path = $inputRels[$index]
            digest = $inputDigests[$index]
        }
    }
    $receiptObject = [ordered]@{
        schema = 2
        jet = [ordered]@{
            path = $jetInfo.path
            version = $jetInfo.version
            identity = $jetInfo.identity
        }
        toolchain = [ordered]@{
            name = $toolchainName
            path = $toolchainPath
            version = $toolchainVersion
            identity = $toolchainIdentity
        }
        target = [ordered]@{
            triple = $targetTriple
            rustc = $rustcCommand
            version = $rustcVersion
            rustc_identity = $rustcIdentity
            sysroot = $sysrootPath
            sysroot_identity = $sysrootInfo.identity
            target_libdir = $targetLibdir
            target_libdir_identity = $targetLibdirInfo.identity
        }
        linker = [ordered]@{
            name = $linkerName
            path = $linkerPath
            version = $linkerVersion
            identity = $linkerIdentity
        }
        lock = [ordered]@{
            path = ".jet/lock"
            digest = $lockDigest
        }
        inputs = @($inputRecords)
        build = [ordered]@{
            entry = $entryRel
            output = $Output
            library = $Library
            kind = $Kind
            profile = $Profile
            loadable = [bool]$Loadable
            command = @($jetArgs)
        }
        artifacts = @($artifactRecords)
    }
    $receiptPath = Join-Path $stage "jet-host.receipt"
    $stampPath = Join-Path $stage "jet-host.stamp"
    $json = $receiptObject | ConvertTo-Json -Compress -Depth 8
    $utf8 = New-Object Text.UTF8Encoding($false)
    [IO.File]::WriteAllText($receiptPath, $json + [Environment]::NewLine, $utf8)
    $receiptDigest = Get-Sha256 $receiptPath
    [IO.File]::WriteAllText($stampPath, ("jet-foreign-host-v2" + [Environment]::NewLine + "receipt=" + $receiptDigest + [Environment]::NewLine), $utf8)

    $null = Get-RealDirectory $destFull "destination"
    foreach ($artifact in $artifacts) {
        $destination = Join-Path $destFull $artifact
        Assert-PublishPath $destination
        Move-Item -LiteralPath (Join-Path $stage $artifact) -Destination $destination
    }
    $receiptDestination = Join-Path $destFull "jet-host.receipt"
    $stampDestination = Join-Path $destFull "jet-host.stamp"
    Assert-PublishPath $receiptDestination
    Assert-PublishPath $stampDestination
    Move-Item -LiteralPath $receiptPath -Destination $receiptDestination
    Move-Item -LiteralPath $stampPath -Destination $stampDestination
    $publishComplete = $true
} finally {
    if ($null -ne $jetProcess -and -not $jetProcess.HasExited) {
        Stop-JetProcessTree $jetProcess.Id
    }
    if (-not $publishComplete -and $null -ne $destFull) {
        Clean-Destination $destFull $Library
    }
    if ($null -ne $stage) {
        Remove-OwnedDirectory $stage
    }
    if ($null -ne $stageProjectDir) {
        Remove-OwnedDirectory $stageProjectDir
    }
    if ($lockOwned -and $null -ne $lockDir) {
        Remove-OwnedFile (Join-Path $lockDir "pid")
        Remove-OwnedFile (Join-Path $lockDir "started")
        Remove-Item -LiteralPath $lockDir -Force -ErrorAction SilentlyContinue
    }
}
