<div align="center">

# 🦊 ForgeFox VPN

**Self-hosted VPN на базе SSH-туннеля (L3). Свой сервер — свои правила.**

[![Release](https://img.shields.io/github/v/release/KiAtsushi-Git/Forge-Fox-VPN?style=flat-square&label=Релиз)](https://github.com/KiAtsushi-Git/Forge-Fox-VPN/releases)
[![Platform](https://img.shields.io/badge/платформы-Windows%20%7C%20Android-blue?style=flat-square)](https://github.com/KiAtsushi-Git/Forge-Fox-VPN/releases)
[![License](https://img.shields.io/badge/лицензия-свободное%20использование-green?style=flat-square)]()

</div>

---

## 📥 Скачать

| Платформа | Файл | Где взять |
|---|---|---|
| 🪟 **Windows** | `ForgeFox-VPN-1.0.0-x64-setup.exe` — обычная установка | [Релизы](https://github.com/KiAtsushi-Git/Forge-Fox-VPN/releases/latest) · папка [`windows/`](windows/) |
| 🪟 **Windows** | `ForgeFox-VPN-1.0.0_x64_en-US.msi` — для тихой/корпоративной установки | [Релизы](https://github.com/KiAtsushi-Git/Forge-Fox-VPN/releases/latest) · папка [`windows/`](windows/) |
| 🤖 **Android** | `ForgeFox-VPN-1.0.apk` | [Релизы](https://github.com/KiAtsushi-Git/Forge-Fox-VPN/releases/latest) · папка [`android/`](android/) |

> Все актуальные сборки всегда лежат на странице [**Releases**](https://github.com/KiAtsushi-Git/Forge-Fox-VPN/releases/latest).

## ✨ Что это

ForgeFox VPN поднимает полноценный L3-туннель через обычный SSH: на сервере ничего
экзотического не нужно — только SSH и root. Никаких проприетарных протоколов и
государственных блокировок на уровне протокола: для провайдера ваш трафик выглядит
как обычное SSH-соединение.

- ⚡ **Быстро** — серверный мост на C (`forgefox-bridge`), батчинг пакетов, AES-GCM
- 🔒 **Приватно** — шифрование SSH, сервер под вашим полным контролем
- 🖥 **Клиенты** — Windows (Tauri, лёгкий инсталлятор) и Android
- 🚀 **Self-Host в один клик** — приложение само настраивает сервер по SSH: установка
  Host (просто VPN) или Provider (веб-панель управления подписками и пользователями)
- 👥 **Панель Provider** — ноды, пользователи, лимиты трафика, ссылки-подписки
- 📋 **Split tunneling** — правила по IP/подсетям/доменам/приложениям, режимы Bypass и Proxy

## 🚀 Быстрый старт

### Windows
1. Скачайте `setup.exe` из [релизов](https://github.com/KiAtsushi-Git/Forge-Fox-VPN/releases/latest) и установите.
2. Откройте вкладку **Self-Host**, добавьте свой сервер (root + пароль) и нажмите «Установить».
3. Или вставьте готовую ссылку `ssh://user:pass@host:port#имя` на вкладке «Серверы».
4. Нажмите кнопку питания — вы защищены.

### Android
1. Скачайте `ForgeFox-VPN-1.0.apk` и установите (разрешите установку из неизвестных источников).
2. «+» → вставьте ссылку `ssh://...` или добавьте подписку.
3. Выберите узел и нажмите кнопку подключения.

### Свой сервер (VPS)
Подойдёт любой Ubuntu/Debian с root-доступом. Установка полностью автоматическая из
приложения (Windows): вкладка **Self-Host → Установить**. Приложение само настроит
sysctl, NAT, TUN и SSH.

## 🗂 Содержимое репозитория

```
android/   → полный исходный код Android-клиента + готовый APK
windows/   → полный исходный код Windows-клиента (Tauri) + установщики (.exe / .msi)
```

Это open-source проект — каждый клиент можно собрать из исходников
(инструкции — в README внутри папок `android/` и `windows/`).

Исходные коды всех компонентов проекта:
- Клиент Windows: [Forge-Fox-VPN/windows](https://github.com/KiAtsushi-Git/Forge-Fox-VPN/tree/main/windows)
- Клиент Android: [Forge-Fox-VPN/android](https://github.com/KiAtsushi-Git/Forge-Fox-VPN/tree/main/android)
- Панель Provider: [Forge-Fox-VPN-Self-Host-Provider](https://github.com/KiAtsushi-Git/Forge-Fox-VPN-Self-Host-Provider)

## ☕ Помочь проекту

Разработка, сервера и поддержка съедают время и деньги. Если ForgeFox оказался
полезен — поддержите автора любой суммой:

<div align="center">

### 💛 [**Поддержать через Ozon Bank (СБП)**](https://finance.ozon.ru/apps/sbp/ozonbankpay/019f4305-135a-75e7-a856-ec182f09a9c4)

*Оплата по СБП — быстро, без комиссии, из любого банка*

</div>

---

<div align="center">

🦊 **ForgeFox VPN** · свой VPN — это просто

</div>
