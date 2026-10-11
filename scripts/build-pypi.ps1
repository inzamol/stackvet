# Build StackVet for PyPI (Wheel & Source Distribution)
$ErrorActionPreference = "Stop"

Write-Host "========================================" -ForegroundColor Cyan
Write-Host " Building StackVet for PyPI (Python)    " -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan

# Ensure output directory exists
$OutDir = "dist/pypi"
if (!(Test-Path $OutDir)) {
    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
}

# 1. Build optimized binary wheel
Write-Host "`n[1/2] Building binary wheel (.whl)..." -ForegroundColor Yellow
maturin build --release --out $OutDir

# 2. Build source distribution
Write-Host "`n[2/2] Building source distribution (.tar.gz)..." -ForegroundColor Yellow
maturin sdist --out $OutDir

Write-Host "`n✓ PyPI packages built successfully in '$OutDir':" -ForegroundColor Green
Get-ChildItem -Path $OutDir | Select-Object Name, Length, LastWriteTime | Format-Table -AutoSize
