$ErrorActionPreference = "Stop"

$env:PATH = [System.Environment]::GetEnvironmentVariable("Path", "Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path", "User") + ";C:\Program Files\Go\bin"
$env:PATH = $env:PATH + ";" + (go env GOPATH) + "\bin"
$env:ANDROID_HOME = "C:\Users\ad\AppData\Local\Android\Sdk"
$env:ANDROID_NDK_HOME = "C:\Users\ad\AppData\Local\Android\Sdk\ndk\25.1.8937393"

Write-Host "Running gomobile init cleanly..."
$procInfo = New-Object System.Diagnostics.ProcessStartInfo
$procInfo.FileName = "gomobile.exe"
$procInfo.Arguments = "init"
$procInfo.UseShellExecute = $false
$keysToRemove = @()
foreach ($key in $procInfo.Environment.Keys) {
    if ($key.StartsWith("=")) {
        $keysToRemove += $key
    }
}
foreach ($key in $keysToRemove) {
    $procInfo.Environment.Remove($key)
}
$procInfo.Environment["ANDROID_HOME"] = $env:ANDROID_HOME
$procInfo.Environment["ANDROID_NDK_HOME"] = $env:ANDROID_NDK_HOME
$procInfo.Environment["PATH"] = $env:PATH
$proc = [System.Diagnostics.Process]::Start($procInfo)
$proc.WaitForExit()

if ($proc.ExitCode -ne 0) {
    Write-Host "gomobile init failed with exit code $($proc.ExitCode)"
    exit 1
}

Write-Host "Building libbox.aar..."
cd sing-box
# We also need to run go run build_libbox cleanly
$procInfo2 = New-Object System.Diagnostics.ProcessStartInfo
$procInfo2.FileName = "go.exe"
$procInfo2.Arguments = "run ./cmd/internal/build_libbox -target android"
$procInfo2.WorkingDirectory = "$PWD"
$procInfo2.UseShellExecute = $false
foreach ($key in $keysToRemove) {
    $procInfo2.Environment.Remove($key)
}
$procInfo2.Environment["ANDROID_HOME"] = $env:ANDROID_HOME
$procInfo2.Environment["ANDROID_NDK_HOME"] = $env:ANDROID_NDK_HOME
$procInfo2.Environment["PATH"] = $env:PATH
$proc2 = [System.Diagnostics.Process]::Start($procInfo2)
$proc2.WaitForExit()

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
