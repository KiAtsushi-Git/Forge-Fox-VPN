$ErrorActionPreference = "Stop"

$env:PATH = [System.Environment]::GetEnvironmentVariable("Path", "Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path", "User")
$env:ANDROID_HOME = "C:\Users\ad\AppData\Local\Android\Sdk"
$env:ANDROID_NDK_HOME = "C:\Users\ad\AppData\Local\Android\Sdk\ndk\25.1.8937393"

Write-Host "Installing gomobile..."
go install golang.org/x/mobile/cmd/gomobile@latest
$env:PATH = $env:PATH + ";" + (go env GOPATH) + "\bin"
gomobile init

Write-Host "Cloning sing-box..."
if (-Not (Test-Path "sing-box")) {
    git clone https://github.com/SagerNet/sing-box.git
}
cd sing-box
git checkout v1.8.0

Write-Host "Building libbox.aar..."
go run ./cmd/internal/build_libbox -target android

Write-Host "Copying libbox.aar to project..."
New-Item -ItemType Directory -Force -Path "../app/libs"
Copy-Item "libbox.aar" -Destination "../app/libs/"
cd ..

Write-Host "Building Rust Core..."
cd rust-core
# Setup NDK path for cargo-ndk
$env:ANDROID_NDK_HOME = "C:\Users\ad\AppData\Local\Android\Sdk\ndk\25.1.8937393"
cargo ndk -t arm64-v8a build --release
cd ..

Write-Host "Downloading Gradle..."
if (-Not (Test-Path "gradle-8.1.1")) {
    Invoke-WebRequest -Uri "https://services.gradle.org/distributions/gradle-8.1.1-bin.zip" -OutFile "gradle.zip"
    Expand-Archive -Path "gradle.zip" -DestinationPath "." -Force
}

Write-Host "Assembling APK..."
.\gradle-8.1.1\gradle-8.1.1\bin\gradle.bat assembleDebug

Write-Host "Done! APK is at app/build/outputs/apk/debug/app-debug.apk"
