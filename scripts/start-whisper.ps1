$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
    throw "Docker CLI was not found. Install Docker Desktop first."
}

docker info *> $null
if ($LASTEXITCODE -ne 0) {
    throw "Docker Desktop is not running or the Docker engine is unavailable."
}

if (-not (Test-Path "whisper.env")) {
    $template = Get-Content "whisper.env.example" -Raw
    $bytes = New-Object byte[] 32
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    $rng.GetBytes($bytes)
    $rng.Dispose()
    $key = [Convert]::ToBase64String($bytes).TrimEnd('=').Replace('+', '-').Replace('/', '_')
    $config = $template.Replace("WHISPER_API_KEY=generated-by-start-script", "WHISPER_API_KEY=$key")
    Set-Content -Path "whisper.env" -Value $config -Encoding ascii
    Write-Host "Created whisper.env with a local API key."
}

docker compose -f docker-compose.cuda.yml up -d
if ($LASTEXITCODE -ne 0) {
    throw "Could not start Whisper. Check Docker Desktop and NVIDIA GPU support."
}

$keyLine = Get-Content "whisper.env" | Where-Object { $_ -match '^WHISPER_API_KEY=' } | Select-Object -First 1
$key = $keyLine -replace '^WHISPER_API_KEY=', ''
$headers = @{ Authorization = "Bearer $key" }
$ready = $false

Write-Host "Waiting for Whisper and the large-v3-turbo model to become ready..."
for ($i = 0; $i -lt 450; $i++) {
    try {
        $null = Invoke-RestMethod -Uri "http://127.0.0.1:9000/v1/models" -Headers $headers -TimeoutSec 3
        $ready = $true
        break
    }
    catch {
        Start-Sleep -Seconds 2
    }
}

if (-not $ready) {
    docker logs windows-voice-input-whisper
    throw "Whisper did not become ready within 15 minutes. Check Docker logs and GPU support."
}

Write-Host "Whisper is ready at http://127.0.0.1:9000"
