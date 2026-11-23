# BatteryHub для AJAZZ AJ159 APEX, Dark Project HS5 V2 и DualSense

Сборка BatteryHub (Rust) с добавленным драйвером мышей AJAZZ AJ-серии на приёмнике SONiX `3151:5007`.

## Что показывает

- **AJAZZ AJ159 APEX** — заряд и состояние связи через 2.4 ГГц приёмник.
- **Dark Project HS5 V2** — заряд по Bluetooth. Донгл 2.4 ГГц заряд не передаёт (чип Actions отдаёт только аудио, медиа-клавиши и канал прошивки), поэтому наушники нужно один раз спарить по Bluetooth. Режим 2.4 ГГц при этом можно не отключать.
- **DualSense** — заряд по USB или Bluetooth (встроенный драйвер BatteryHub).
- **AULA F75 / F87 Pro / F99** — заряд через 2.4 ГГц приёмник (отчёт `0x13`, команда `0x4A`).
- **Attack Shark** — мыши на Xenta (`1D57:FA60`) и на приёмниках Rongyuan (`3151:402D` и другие); протокол общий с AJAZZ-приёмниками.
- Любые другие устройства, которые BatteryHub умеет читать.

Проценты рисуются на панели задач рядом с часами; при наведении видно название устройства и состояние. Если панель задач вертикальная или нестандартная, приложение переключается на значки в трее.

## Запуск

Готовый установщик: `release\BatteryHub-Taskbar-Setup-v1.4.1.exe`.

Или из исходников:

```powershell
cargo build --release
.\target\release\batteryhub-taskbar.exe
```

Установка в `%LOCALAPPDATA%\Programs\BatteryHub\Taskbar` с автозапуском:

```powershell
.\release\BatteryHub-Taskbar-Setup-v1.4.1.exe /install /S /autostart=yes
```

## Язык

Правый клик по процентам → **Язык** → **English** или **Русский**. Выбор сохраняется в
`HKCU\Software\BatteryHub\Taskbar\Language`. При первом запуске язык берётся из языка Windows.

## Что изменено

- Добавлен `src/drivers/ajazz.rs`: опрос `SET_FEATURE 0xF7`, затем `GET_FEATURE 0x05`, заряд в байте 3.
- Драйвер подключён в цепочку `src/device.rs` до универсальных драйверов.
- Добавлен `src/i18n.rs`: русский/английский интерфейс, переключение в меню, сохранение выбора.
- Наушники и геймпад используют существующие драйверы BatteryHub (Bluetooth и Sony).

## Лицензия

GPL-3.0-or-later. Основа — [BatteryHub (Rust)](https://github.com/ferloom/Wireless-Device-Battery-Monitor), переписанный с C++ BatteryHub by krisgfx9. Полный текст лицензии — в `LICENSE`.
