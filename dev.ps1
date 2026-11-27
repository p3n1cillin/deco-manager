# Run Deco Manager in dev mode.
# Adds rustup's default cargo folder to PATH only if cargo isn't already available.
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoBin = Join-Path $HOME ".cargo/bin"
    if (Test-Path $cargoBin) { $env:Path = "$cargoBin;$env:Path" }
}
Set-Location $PSScriptRoot
if (-not (Test-Path node_modules)) { npm install }
npx tauri dev
