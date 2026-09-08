@echo off
echo Building rust-core...
cd rust-core
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86 -t x86_64 -o ../app/src/main/jniLibs build --release
cd ..
echo Building Android app...
call gradle-8.1.1\bin\gradle.bat assembleDebug
echo Build finished! APK is located in app/build/outputs/apk/debug/app-debug.apk
