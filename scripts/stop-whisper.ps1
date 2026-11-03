$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
docker compose -f docker-compose.cuda.yml down
if ($LASTEXITCODE -ne 0) {
    throw "Could not stop Whisper."
}
