# ForgeFox VPN SSH — Desktop Windows Client

Десктопный VPN клиент для Windows на базе SSH-туннелей. Дизайн вдохновлён WireSock.

## Стек

| Слой | Технология |
|------|-----------|
| GUI оболочка | [Tauri 2](https://tauri.app) |
| VPN ядро | Rust (`russh`, `wintun`, `tokio`) |
| Фронтенд | Vanilla HTML / CSS / JS |
| Конфиг | JSON (`%APPDATA%/ForgeFoxVPN/`) |

## Возможности

- 🔒 SSH L3 туннель (тот же механизм, что в Android-версии)
- ⚡ Автопереподключение при разрыве
- 🌐 Гибкие правила исключений в **2 режимах**:
  - **Bypass** — весь трафик через VPN, кроме списка
  - **Proxy** — только список через VPN
- 📋 Добавление правил по: IP, CIDR подсети, домену (с DNS-резолвом), пути к .exe
- 🗂 Управление серверами: ручное добавление, SSH-ссылки (`ssh://`), подписки (HTTP)
- 🎨 4 темы: Dark, Midnight, Ocean, Light
- 📊 Статистика трафика и аптайм
- 🔔 Сворачивание в трей

## Требования

- Windows 10 или новее (x64)
- Rust (`rustup.rs`) + Visual Studio C++ Build Tools
- Node.js ≥ 18 (для `pnpm / npm`)
- [Tauri CLI v2](https://tauri.app/start/create-project/)

## Установка зависимостей

```bash
# Rust
rustup target add x86_64-pc-windows-msvc

# Tauri CLI
cargo install tauri-cli --version "^2"
```

## Разработка

```bash
cd E:/GitLab/forgefoxvpn-ssh-main
cargo tauri dev
```

> **Важно:** WinTun требует прав администратора для создания виртуального сетевого адаптера.
> Запускайте `cargo tauri dev` от имени администратора.

## Сборка релиза

```bash
cargo tauri build
# .msi и .exe installer появятся в src-tauri/target/release/bundle/
```

## Формат SSH-ссылки

```
ssh://username:password@host:port#Имя сервера
```

Пример:
```
ssh://root:mypassword@1.2.3.4:22#My VPN
```

## Как работает туннель

1. Создаётся WinTun виртуальный адаптер (`ForgeFoxVPN`)
2. IP SSH-сервера добавляется в таблицу маршрутов через реальный шлюз (защита от петли)
3. Маршрут по умолчанию перенаправляется через WinTun
4. SSH-соединение устанавливается с сервером, запускается Python-скрипт TUN-моста
5. Двунаправленная передача пакетов: WinTun ↔ SSH-канал (с 2-байтовым prefixом длины)

## Правила исключений

### Bypass-режим
Весь трафик идёт через VPN, кроме адресов в списке.
Список адресов (IP/CIDR/домены) маршрутизируется через реальный шлюз.

### Proxy-режим
Трафик по умолчанию идёт напрямую, через VPN — только адреса из списка.

> ⚠️ Правила для приложений (`.exe`) требуют Windows Filtering Platform (WFP) — 
> аналогично WireSock. Текущая реализация использует маршруты таблицы маршрутизации.
> Полная поддержка WFP планируется в следующих версиях.

## Структура проекта

```
forgefoxvpn-ssh-main/
├── src-tauri/
│   ├── src/
│   │   ├── lib.rs              # Tauri app setup, tray icon
│   │   ├── main.rs             # Entry point
│   │   ├── commands.rs         # Tauri IPC commands
│   │   ├── config/
│   │   │   ├── mod.rs          # Config load/save
│   │   │   └── models.rs       # Data models
│   │   └── vpn/
│   │       ├── mod.rs          # VPN thread management
│   │       ├── ssh_vpn.rs      # SSH tunnel core
│   │       ├── tun.rs          # WinTun adapter
│   │       └── routing.rs      # Windows route table
│   ├── Cargo.toml
│   ├── build.rs
│   └── tauri.conf.json
└── src/
    ├── index.html
    ├── css/
    │   ├── main.css
    │   └── themes/
    │       ├── dark.css        # Default dark theme
    │       ├── midnight.css    # Pure blacks + purple accent
    │       ├── ocean.css       # Deep blue
    │       └── light.css       # Light mode
    └── js/
        ├── api.js              # Tauri invoke() wrappers
        └── app.js              # UI logic
```

## Лицензия

MIT
