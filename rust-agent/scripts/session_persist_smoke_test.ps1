# Session Persistence Smoke Test
# Requires: PowerShell, cargo, curl
$ErrorActionPreference = "Stop"

$port = 3999
$sessionDir = "$env:USERPROFILE\.rust-agent\sessions"

# Clean up old sessions
if (Test-Path $sessionDir) {
    Remove-Item "$sessionDir\*.json" -Force -ErrorAction SilentlyContinue
}

# Step 1: Start server in background
Write-Host "[1/6] Starting server on port $port..."
$proc = Start-Process -FilePath "cargo" -ArgumentList "run","-p","rust-agent-server","--","--port","$port" -PassThru -WindowStyle Hidden
Start-Sleep -Seconds 15

try {
    # Step 2: Create session
    Write-Host "[2/6] Creating session..."
    $resp = Invoke-RestMethod -Uri "http://localhost:$port/sessions" -Method POST
    $id = $resp.id
    Write-Host "      Created session: $id"
    if (-not $id) { throw "No session ID returned" }

    # Step 3: Verify file exists
    Write-Host "[3/6] Checking session file on disk..."
    $files = Get-ChildItem "$sessionDir\*.json" -ErrorAction SilentlyContinue
    if ($files.Count -ne 1) { throw "Expected 1 session file, found $($files.Count)" }
    Write-Host "      Found: $($files[0].Name)"

    # Step 4: Restart server
    Write-Host "[4/6] Restarting server..."
    Stop-Process -Id $proc.Id -Force
    Start-Sleep -Seconds 2
    $proc = Start-Process -FilePath "cargo" -ArgumentList "run","-p","rust-agent-server","--","--port","$port" -PassThru -WindowStyle Hidden
    Start-Sleep -Seconds 15

    # Step 5: Verify recovery
    Write-Host "[5/6] Verifying session recovery..."
    $resp2 = Invoke-RestMethod -Uri "http://localhost:$port/sessions/$id" -Method GET
    if ($resp2.id -ne $id) { throw "Session ID mismatch after recovery" }
    Write-Host "      Recovered session: $($resp2.id) (messages=$($resp2.message_count))"

    # Step 6: Delete session
    Write-Host "[6/6] Deleting session..."
    Invoke-RestMethod -Uri "http://localhost:$port/sessions/$id" -Method DELETE
    $filesAfter = Get-ChildItem "$sessionDir\*.json" -ErrorAction SilentlyContinue
    if ($filesAfter.Count -ne 0) { throw "Expected 0 session files after delete, found $($filesAfter.Count)" }
    Write-Host "      Session file removed."

    Write-Host "`n✅ All smoke tests passed."
} finally {
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
}
