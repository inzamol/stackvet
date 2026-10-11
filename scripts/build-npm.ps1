# Build StackVet for npm (Node.js Package & Tarball)
$ErrorActionPreference = "Stop"

Write-Host "========================================" -ForegroundColor Cyan
Write-Host " Building StackVet for npm (Node.js)    " -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan

# Ensure native release binary is built
Write-Host "`n[1/3] Building native Rust release binary..." -ForegroundColor Yellow
cargo build -p stackvet --release --locked

# Ensure output directory exists
$OutDir = "dist/npm"
if (!(Test-Path $OutDir)) {
    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
}

# Copy latest README and LICENSE
Copy-Item -Path "README.md" -Destination "packages/npm/README.md" -Force
Copy-Item -Path "LICENSE" -Destination "packages/npm/LICENSE" -Force

# Copy native binary into package
Write-Host "`n[2/3] Bundling native binary into npm package..." -ForegroundColor Yellow
Push-Location "packages/npm"
try {
    node scripts/copy-binary.js

    # Package into tarball (.tgz)
    Write-Host "`n[3/3] Packaging npm tarball (.tgz)..." -ForegroundColor Yellow
    npm pack --pack-destination ../../dist/npm
}
finally {
    Pop-Location
}

Write-Host "`n✓ npm package built successfully in '$OutDir':" -ForegroundColor Green
Get-ChildItem -Path $OutDir | Select-Object Name, Length, LastWriteTime | Format-Table -AutoSize
