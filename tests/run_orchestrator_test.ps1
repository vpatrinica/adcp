# Orchestrator end-to-end test script for Windows
# Usage: .\tests\run_orchestrator_test.ps1

$Repo = Get-Location
$TmpDir = "$Repo\deployment\tmp"
$LogDir = "$Repo\deployment\log"
$OrchConf = "$TmpDir\orchestrator.toml"
$LogFileOut = "$LogDir\adcp-orchestrator.stdout.log"
$LogFileErr = "$LogDir\adcp-orchestrator.stderr.log"
$PidFile = "$TmpDir\orchestrator.pid"

# Stop any existing adcp processes
Write-Host "Stopping any existing adcp processes..."
Get-Process adcp -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 2

# Cleanup previous artifacts
Write-Host "Cleaning up previous artifacts..."
if (Test-Path "$Repo\deployment") {
    Remove-Item -Recurse -Force "$Repo\deployment" -ErrorAction SilentlyContinue
}

# Create directories
New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null
New-Item -ItemType Directory -Path $LogDir -Force | Out-Null

# Write orchestrator config
$ConfigContent = @"
service_name = "adcp-orchestrator"
mode = "Orchestrator"
serial_port = "NUL"
data_directory = "./deployment/data"
file_stability_seconds = 5
"@
Set-Content -Path $OrchConf -Value $ConfigContent

# Build project
Write-Host "Building project..."
cargo build --release

# Start orchestrator
Write-Host "Starting orchestrator..."
$Process = Start-Process -FilePath ".\target\release\adcp.exe" -ArgumentList $OrchConf -RedirectStandardOutput $LogFileOut -RedirectStandardError $LogFileErr -PassThru -WindowStyle Hidden
$Process.Id | Set-Content -Path $PidFile
Write-Host "Started orchestrator with PID $($Process.Id)"

# Let it run briefly
Write-Host "Running for 10 seconds..."
Start-Sleep -Seconds 10

# Request graceful shutdown
# On Windows, we can't easily send SIGINT to a background process without a console.
# We'll use Stop-Process which is like kill -9.
Write-Host "Stopping orchestrator..."
Stop-Process -Id $Process.Id -Force

Start-Sleep -Seconds 2

# Cleanup leftover pid files
Write-Host "Cleaning up leftover files..."
if (Test-Path "$TmpDir\adcp-*.pid") {
    Remove-Item -Force "$TmpDir\adcp-*.pid"
}
if (Test-Path $PidFile) {
    Remove-Item -Force $PidFile
}

# Show results
Write-Host "`n--- deployment/to_process ---"
if (Test-Path "$Repo\deployment\to_process") {
    Get-ChildItem -Path "$Repo\deployment\to_process" -Recurse
} else {
    Write-Host "no to_process"
}

Write-Host "`n--- deployment/processed ---"
if (Test-Path "$Repo\deployment\processed") {
    Get-ChildItem -Path "$Repo\deployment\processed" -Recurse
} else {
    Write-Host "no processed"
}

Write-Host "`n--- deployment/data ---"
if (Test-Path "$Repo\deployment\data") {
    Get-ChildItem -Path "$Repo\deployment\data" -Recurse
} else {
    Write-Host "no data"
}

# Final check for e2e-test.pid (from common.rs usage)
if (Test-Path "$TmpDir\e2e-test.pid") {
    Write-Host "WARNING: e2e-test.pid still exists!"
}
