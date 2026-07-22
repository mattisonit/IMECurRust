[CmdletBinding()]
param(
    [switch]$SkipTests
)

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "Cargo를 찾을 수 없습니다. Rust의 MSVC 툴체인을 먼저 설치하세요."
}

if (-not $SkipTests) {
    Write-Host "[1/3] 테스트 실행"
    cargo test
    if ($LASTEXITCODE -ne 0) { throw "cargo test 실패" }
}

Write-Host "[2/3] Release 빌드"
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build --release 실패" }

Write-Host "[3/3] 배포 폴더 구성"
$dist = Join-Path $PSScriptRoot "dist"
New-Item -ItemType Directory -Force -Path $dist | Out-Null

$sourceExe = Join-Path $PSScriptRoot "target\release\ime-cursor.exe"
$targetExe = Join-Path $dist "IMECurRust.exe"
Copy-Item -Force $sourceExe $targetExe
Copy-Item -Force (Join-Path $PSScriptRoot "IMECur.ini") $dist

foreach ($name in @("IMEE.wav", "IMEJ.wav", "IMEK.wav")) {
    $sound = Join-Path $PSScriptRoot ("assets\" + $name)
    if (Test-Path $sound) {
        Copy-Item -Force $sound $dist
    }
}

Write-Host ""
Write-Host "완료: $targetExe"
