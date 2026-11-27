# Run the app in dev mode (hot-reloads the UI; rebuilds Rust on change)
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
Set-Location $PSScriptRoot
npx tauri dev
