<div align="center">

<img src="assets/logo.png" width="96" alt="Prosvet" />

# Prosvet

**Немного света сквозь блокировки.**
Открытое приложение для Windows, которое возвращает YouTube, Discord и другие сервисы —
без VPN, без серверов и без подписок.

<img src="docs/home.png" width="760" alt="Главный экран Prosvet" />

</div>

## Что это

Prosvet — красивая оболочка на Rust над проверенными движками обхода DPI
[zapret2 / zapret](https://github.com/bol-van/zapret2) и стратегиями
[Flowseal](https://github.com/Flowseal/zapret-discord-youtube).
Вместо двадцати `.bat`-файлов — одна кнопка.

- **Сам подбирает стратегию.** При первом запуске проверяет, что заблокировано у вашего провайдера,
  перебирает ~30 стратегий (zapret2 на Lua + все стратегии Flowseal) и оставляет лучшую.
- **Следит за работой.** Сторож периодически проверяет доступ и сам переключается,
  если провайдер что-то поменял.
- **Честная проверка.** Тестер не просто ждёт ответа, а докачивает данные —
  так ловится характерная для ТСПУ «заморозка» соединения на ~16 КБ.
- **Всё локально.** Трафик никуда не уходит: пакеты хитро нарезаются прямо на вашем компьютере.
- **Не мешает.** Живёт в трее, стартует вместе с Windows без запросов UAC,
  движок завершается вместе с приложением (Job Object) — никаких «висящих» winws.
- **Видит конфликты.** Находит запущенные службы zapret/GoodbyeDPI и предлагает их остановить.
- **Обновляется.** Свежие версии zapret2 и стратегий Flowseal — одной кнопкой, свои списки не трогаются.
- **Открывает ИИ.** Gemini, AI Studio, NotebookLM, ChatGPT, Claude, Grok и Copilot сами не пускают
  пользователей из России — это геоблок, а не РКН, и zapret его не снимает. Prosvet включает «умный DNS»
  (Comss / GeoHide, без Xbox DNS): только эти домены идут через зарубежный прозрачный прокси,
  переписка остаётся зашифрованной (TLS), прокси видит лишь имя сайта.

<p align="center">
  <img src="docs/onboarding.png" width="49%" alt="Приветствие" />
  <img src="docs/home-light.png" width="49%" alt="Светлая тема" />
</p>
<p align="center">
  <img src="docs/strategies.png" width="760" alt="Стратегии" />
</p>

## Установка

Скачайте `Prosvet_x64-setup.exe` из [релизов](../../releases) и запустите.

> Microsoft Defender считает движок winws «HackTool» — это ложное срабатывание, так помечают
> все инструменты обхода DPI. Prosvet при первом запуске предложит добавить исключение.

## Сборка из исходников

Нужны: Rust (stable, MSVC), Node.js 18+, Windows 10/11.

```powershell
npm install
npm run engine      # скачать winws/winws2, стратегии и списки в src-tauri/resources/engine
npm run build       # установщик: src-tauri/target/release/bundle/nsis/
```

Для разработки интерфейса Tauri не нужен: откройте `ui/index.html` через любой локальный
сервер — подключится мок бэкенда. Состояния переключаются через адрес:
`index.html#dark&run`, `#light&page=strategies`, `#dark&onb=0`.

Тесты (включая прогон **всех** стратегий через `winws --dry-run`):

```powershell
cd src-tauri; cargo test
```

## Как устроено

```
ui/                    интерфейс: HTML/CSS/JS без сборщика, сцена «Луч» на canvas
src-tauri/src/
  controller.rs        включение, автоподбор, сторож
  strategy.rs          стратегии zapret2 + разбор .bat от Flowseal на лету
  engine.rs            запуск winws/winws2 в Job Object, журнал
  tester.rs            проверка YouTube / Discord / Google / Cloudflare
  system.rs            конфликты, автозапуск, Defender, TCP timestamps
  aidns.rs             разблокировка ИИ: локальный DNS + NRPT + DoH к умным DNS
  updater.rs           обновление движков из релизов GitHub
scripts/               загрузка движков, генерация иконок и картинок установщика
```

Данные хранятся в `%LOCALAPPDATA%\Prosvet`: движки, списки, настройки и `prosvet.log`.

### Разблокировка ИИ

Prosvet поднимает DNS-сервер на `127.0.0.153:53` и через правила NRPT Windows направляет к нему
**только** домены ИИ-сервисов. Сервер спрашивает умный DNS по DoH (с переключением на запасной провайдер),
а записи AAAA и HTTPS гасит, чтобы браузер не ушёл напрямую по IPv6. Правила снимаются при выходе
и чистятся при следующем запуске, если приложение завершилось аварийно.

VPN-клиенты в TUN-режиме (sing-box, Throne, Happ…) перехватывают весь DNS — Prosvet это замечает
и подсказывает: с включённым VPN разблокировка не нужна.

Диагностика: `PROSVET_DOH_PROXY=http://user:pass@host:port` — пустить DoH через прокси,
`PROSVET_AI_PORT=5454` — поднять сервер на другом порту и опрашивать его вручную.

## Благодарности

Вся магия обхода — заслуга [bol-van](https://github.com/bol-van) и
[Flowseal](https://github.com/Flowseal). Полный список компонентов и лицензий — в
[THIRD_PARTY.md](THIRD_PARTY.md).

## Лицензия

[MIT](LICENSE)
