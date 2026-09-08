$ErrorActionPreference = "Stop"

$env:PATH = [System.Environment]::GetEnvironmentVariable("Path", "Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path", "User")
$env:ANDROID_HOME = "C:\Users\ad\AppData\Local\Android\Sdk"
$env:ANDROID_NDK_HOME = "C:\Users\ad\AppData\Local\Android\Sdk\ndk\25.1.8937393"
$env:PATH = $env:PATH + ";" + (go env GOPATH) + "\bin"

Write-Host "Installing sagernet gomobile..."
go install github.com/sagernet/gomobile/cmd/gomobile@latest
gomobile init

cd sing-box
Write-Host "Building libbox.aar..."
go run ./cmd/internal/build_libbox -target android

Write-Host "Copying libbox.aar to project..."
New-Item -ItemType Directory -Force -Path "../app/libs" -ErrorAction SilentlyContinue
Copy-Item "libbox.aar" -Destination "../app/libs/"
cd ..

Write-Host "Building Rust Core..."
cd rust-core
cargo ndk -t arm64-v8a build --release
cd ..

Write-Host "Assembling APK..."
.\gradle-8.1.1\gradle-8.1.1\bin\gradle.bat assembleDebug

Write-Host "Done! APK is at app/build/outputs/apk/debug/app-debug.apk"
