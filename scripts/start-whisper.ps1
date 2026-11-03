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
    [System.Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
    $key = [Convert]::ToBase64String($bytes).TrimEnd('=').Replace('+', '-').Replace('/', '_')
    $config = $template.Replace("WHISPER_API_KEY=generated-by-start-script", "WHISPER_API_KEY=$key")
    Set-Content -Path "whisper.env" -Value $config -Encoding ascii
    Write-Host "Created whisper.env with a local API key."
}

docker compose -f docker-compose.cuda.yml up -d
if ($LASTEXITCODE -ne 0) {
    throw "Could not start Whisper. Check Docker Desktop and NVIDIA GPU support."
}

Write-Host "Whisper is starting. First launch downloads large-v3-turbo."
Write-Host "API: http://127.0.0.1:9000"
