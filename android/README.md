# ForgeFox VPN — Android Client 🤖

Клиент ForgeFox VPN для Android. Поднимает L3-туннель через SSH — тот же протокол,
что и десктопный клиент, поэтому работает с любым сервером, установленным через
десктопное приложение (Self-Host → Host / Provider).

## ✨ Возможности

- 🔗 Добавление серверов по ссылке `ssh://user:pass@host:port#имя` и подпискам
- 📶 Тот же серверный мост, что у десктопа: быстрый `forgefox-bridge` + Python-fallback с NAT
- 📋 Split tunneling: исключения по приложениям, доменам, IP/подсетям (Bypass / Proxy)
- ⚡ Пинг узлов, переключение сервера на лету без разрыва (hotswap)
- 📊 Счётчик трафика и статус в уведомлении
- 🎨 Тёмный glass-дизайн

## 🛠 Сборка из исходников

Требуется: Android SDK, NDK (25.x), Rust (`cargo` + `cargo-ndk`), JDK 17.

```bash
# 1. Собрать rust-core под нужные ABI (результат ляжет в app/src/main/jniLibs)
cd rust-core
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86 -t x86_64 -o ../app/src/main/jniLibs build --release

# 2. Собрать APK
cd ..
./gradle-8.1.1/bin/gradle.bat assembleDebug   # Windows
# или: gradle assembleDebug                    # если gradle установлен глобально
```

Готовый APK: `app/build/outputs/apk/debug/`.

> В репозитории уже лежат предсобранные `.so` в `app/src/main/jniLibs` — для сборки
> APK без изменений NDK не обязателен.

## 📦 Установка

Скачайте готовый `ForgeFox-VPN-1.0.apk` из [релизов](https://github.com/KiAtsushi-Git/Forge-Fox-VPN/releases)
или папки [`android/`](.) и установите, разрешив установку из неизвестных источников.

## 🔗 Связанные проекты

- **Windows-клиент:** [Forge-Fox-VPN/windows](https://github.com/KiAtsushi-Git/Forge-Fox-VPN/tree/main/windows)
- **Панель Provider (Self-Host):** [Forge-Fox-VPN-Self-Host-Provider](https://github.com/KiAtsushi-Git/Forge-Fox-VPN-Self-Host-Provider)
- **Исходники (GitLab):** [forgefoxvpn-android](https://gitlab.com/KiAtsushi-Git/forgefoxvpn-android)

## ☕ Поддержать проект

[Помочь через Ozon Bank (СБП)](https://finance.ozon.ru/apps/sbp/ozonbankpay/019f4305-135a-75e7-a856-ec182f09a9c4)
