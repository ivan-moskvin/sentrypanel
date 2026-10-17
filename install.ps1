# Installs sentrypanel from the latest release. After that the utility looks
# after itself: sentrypanel update to update, sentrypanel uninstall to remove.
#
#   irm https://raw.githubusercontent.com/ivan-moskvin/sentrypanel/main/install.ps1 | iex

$ErrorActionPreference = "Stop"

$repo = "ivan-moskvin/sentrypanel"
$binDir = if ($env:SENTRYPANEL_BIN_DIR) { $env:SENTRYPANEL_BIN_DIR } else { Join-Path $env:LOCALAPPDATA "sentrypanel" }

$arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    "AMD64" { "amd64" }
    "ARM64" { "arm64" }
    default { throw "Unknown architecture: $($env:PROCESSOR_ARCHITECTURE) — build it from source." }
}

$asset = "sentrypanel_windows_$arch.exe"
$base = "https://github.com/$repo/releases/latest/download"
$exe = Join-Path $binDir "sentrypanel.exe"
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $tmp | Out-Null

try {
    Write-Host "==> Downloading $asset"
    Invoke-WebRequest -Uri "$base/$asset" -OutFile "$tmp\sentrypanel.exe"
    Invoke-WebRequest -Uri "$base/checksums.txt" -OutFile "$tmp\checksums.txt"

    $want = (Get-Content "$tmp\checksums.txt" |
        Where-Object { $_ -match "\s\*?$([regex]::Escape($asset))$" } |
        ForEach-Object { ($_ -split '\s+')[0] })
    if (-not $want) { throw "The release has no checksum for $asset." }

    $got = (Get-FileHash "$tmp\sentrypanel.exe" -Algorithm SHA256).Hash
    if ($got -ne $want.ToUpper()) { throw "The checksum does not match — the download is broken." }

    New-Item -ItemType Directory -Force -Path $binDir | Out-Null
    # A running panel holds its exe: it cannot be overwritten, but it can be
    # renamed out of the way.
    if (Test-Path $exe) {
        Remove-Item -Force "$exe.old" -ErrorAction SilentlyContinue
        Move-Item -Force $exe "$exe.old"
    }
    Move-Item -Force "$tmp\sentrypanel.exe" $exe
    Remove-Item -Force "$exe.old" -ErrorAction SilentlyContinue
    Write-Host "==> Installed to $exe"

    # The directory goes into the user PATH — otherwise the sentrypanel command
    # will not be found.
    $current = [Environment]::GetEnvironmentVariable("Path", "User")
    $entries = @()
    if ($current) { $entries = $current -split ";" | Where-Object { $_ } }
    if ($entries -notcontains $binDir) {
        [Environment]::SetEnvironmentVariable("Path", (($entries + $binDir) -join ";"), "User")
        Write-Host "==> The directory was added to PATH — it works in a new terminal"
    }
    if (($env:Path -split ";") -notcontains $binDir) {
        $env:Path = "$env:Path;$binDir"
    }

    # A service installed earlier still runs the previous binary: installing it
    # again restarts it on this one.
    $run = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "sentrypanel" -ErrorAction SilentlyContinue
    if ($run) {
        & $exe service install
    } else {
        $config = & $exe config 2>$null
        Write-Host ""
        Write-Host "Next:"
        if (-not (Test-Path $config)) {
            Write-Host "  sentrypanel config init       create $config"
        }
        Write-Host "  sentrypanel once              check the config and the token"
        Write-Host "  sentrypanel service install   start the panel at login, and now"
    }
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
