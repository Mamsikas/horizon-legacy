# Horizon Launcher для Windows 7 x64: состояние реализации

Это отчёт о проверяемой части ТЗ №1, а не заявление о завершённой поддержке Windows 7.
Приоритет после уточнения задания — Windows 7 SP1 x64, Atom N455/GMA 3150 и 2 ГБ ОЗУ.
Windows 7 x64 с Radeon HD 4570 также требует отдельного аппаратного прогона.
32-битная сборка оставлена за пределами этой реализации как разрешённая дополнительная цель.

## База и границы изменений

Работа выполняется в форке `Mamsikas/horizon-legacy`.
Базовый HEAD перед изменениями: `59aa0e53df43fe087849250c7fd91de3b8636f2e`.
Указанный в ТЗ commit `36fc4fa187a4e1f3664c8891dc91e2c9e5a5b911` существует, но не является
HEAD этого checkout. Между ним и базой в проверяемой области изменены
`launcher/src/backend.rs`, `launcher/src/main.rs` и `launcher/src/nightly.rs`.
Номера строк из ТЗ не использовались как контракт.

Современный `launcher/src/main.rs` и его GPUI-страницы сохраняются. Главный launcher по
умолчанию собирается с `gui`; отдельный `horizon-launcher-win7` собирается с
`--no-default-features --features win7`. Корневые README и roadmap не изменяются.
Протокол игры, миры и общие графические настройки не изменяются.

Native UI использует USER32 controls и GDI для текста. Формы описываются существующим
`clap`-деревом CLI, проверяются тем же parser и запускают тот же CLI/core в дочернем
worker-процессе. Вторая реализация установки, авторизации, миров или модов не создаётся.
Длинные операции не выполняются в оконном цикле. Результаты показываются текстом/JSON;
наличие формы не означает, что соответствующий сценарий уже прошёл приёмку на Win7.

## L0: карта исходных ограничений

| Подсистема | Исходные модули | Ограничение Win7/GMA 3150 | Принятое решение / остаток |
|---|---|---|---|
| Современный UI | `launcher/Cargo.toml`, `main.rs`, `*_page.rs`, `gpui-fast-windows 0.1.13` | `D3D11CreateDevice` с FL 11_1/11_0/10_1, `CreateDXGIFactory2`, `IDXGIFactory6`, compute, новые DirectWrite/DPI/Composition API | Полностью исключить GPUI из legacy graph; отдельный USER32/GDI entrypoint |
| Общий core | `lib.rs`, `backend.rs`, `cli.rs` | Обязательные GPUI dependencies даже для библиотеки; Linux `pictures` делает GPU probe | Optional dependencies и `required-features`; GPU probe только при `gui`; core остаётся общим |
| Microsoft и лицензия | `account.rs`, `account/session.rs`; `microsoft.rs` рисует кнопку | Нужны реальные TLS, Device Code, приложение Azure и ownership; legacy ОС сама это не подтверждает | Использовать существующие `SignIn`, token store и owner gate; не обходить отказ. Пользователь подтвердил отсутствие client ID |
| Версии и архитектура | `backend::{PLATFORMS, OWN_ASSETS, ASSET, LAUNCHER_ASSET, GameArchitecture}` | Современный Windows x64 archive не является Win7 build; x86 исходно отсутствует | Отдельные имена `horizon-win7-x64.zip` и `horizon-launcher-win7-x64.zip`, без fallback к modern; маркер привязан к SHA-256 клиента |
| Ресурсы | `sources.rs`, `mojang.rs`, `backend::preparer`, `horizon-prepare` | Подготовка вызывает Java 25 и код vanilla; готовые exporter classes не устраняют JVM | Отдельный pinned Liberica JRE/JDK candidate; существующие hash/ownership checks; fresh install остаётся непроверенным |
| Память подготовки | `horizon-prepare/{java,worker,pool}.rs` | Несколько JVM и исходный `-Xmx2G` непригодны как гарантированный бюджет для машины с 2 ГБ | Worker native UI по умолчанию задаёт один job и cap 512 MiB; пользовательский override сохраняется. Достаточность cap требует vanilla-прогона |
| Запуск | `backend::{client_as,client_command,spawn}`, `instances.rs`, `launch.rs`, `windows_runtime.rs` | Win7 ABI, собственный home/resources/session, стабильный путь запуска | Общая сборка `Command`, существующие owner/prepared checks и runtime guard; legacy renderer env только дочернему процессу |
| Моды и паки | `packs.rs`, `modpacks.rs`, `browser/*`, `native_mods.rs`, `rust_toolchain.rs` | Content downloads отделимы от UI; native mods пересобирают весь клиент современным Rust target | Формы существующих операций; native build явно отказывает до появления Win7 toolchain/ABI/cache. Полный parity не заявлен |
| Файлы и Shell | `shortcut.rs`, `desktop_seams.rs`, `installer.rs` | Windows wrappers могут импортировать WinRT/combase; удаление должно оставаться восстановимым | Прямой Win7 Shell/ole32 ABI для links/recycle. Длинные/UNC recycle paths пока отвергаются с сохранением исходных файлов |
| Установка/обновление | `backend.rs`, `update_handoff.rs`, `update_recovery.rs`, `installer.rs` | Замена запущенного EXE, прерывание commit, архитектурная путаница | Отдельный launcher payload name, staging/rollback/recovery; настоящая Win7 self-update и installer-приёмка ещё нужны |
| Rust ABI и CPU | Rust `std`, `getrandom`, transitive Windows bindings | Обычный современный `*-pc-windows-*` не гарантирует NT 6.1; N455 не имеет SSE4/POPCNT/AVX | `x86_64-win7-windows-gnu`, pinned nightly и `build-std`, CPU `x86-64`/SSE2; фактический PE audit обязателен |

Проверка импортов нужна для всего итогового EXE, а не только исходного UI. Уже при
первой линковке аудит обнаружил реальные транзитивные WinRT/combase imports; их нельзя
считать допустимыми только из-за отсутствия GPUI в dependency tree.
Прямые ole32 bindings устраняют конфликт импортов разных поколений Windows wrappers;
у `ipconfig` отключается неиспользуемая feature `computer`, тянувшая Windows Registry/WinRT
wrappers. Чтение DNS-адаптеров и игровой сетевой протокол этим не изменяются.

## Функциональный паритет

В таблицах «форма есть» означает доступ к существующей операции core через native controls.
Статус каждого действия на физических Win7-машинах пока **НЕ ПРОВЕРЕНО**.
Unit/contract tests parser или core не заменяют этот статус.

| Современная страница `Page` | Доступные native операции | Что пока не перенесено / ограничено |
|---|---|---|
| `Play` / Home | `play`; `prepare`; `account list/select/refresh/sign-out`; отдельная Device Code панель; операции `instance`; журналы | Нет подтверждённого запуска реального Win7 клиента; нет полного Home visual overview и всех GUI-only действий; внешний браузер/реальный вход не проверены |
| `Versions` | `versions`, `install`, `remove`, `import-build`, `update --check`, `update`; выбранная версия через `settings` | Подписанные Win7 client/launcher releases ещё должны быть опубликованы; unverified import нельзя выдавать за совместимый клиент |
| `Mods` | `mods list/search/check/install/update/remove/enable/disable/add-github/update-github`; `packs`; `modpack` | Нет визуальных карточек/preview; native compilation явно не завершена; часть Java/modpack операций зависит от JVM и клиента |
| `Servers` | `servers list/create/set-up/run/forget`; `server`; `multiplayer`; `play --server/--join`; ввод строк в stdin worker из native окна | Реальная интерактивная server console и подключение требуют совместимого клиента/сервера; не проверены на Win7 |
| `Wardrobe` | `legacy wardrobe-list/equip/unequip/visible/preferences/claim`; существующие offline skin/cape операции `account skin` | Используются account/wardrobe core и offline queue; сетевой сценарий требует входа. 3D preview отсутствует |
| `Shop` | `legacy shop-catalogue/history/quote/checkout/claim` через существующие pricing и service API | Checkout возвращает проверенный HTTPS URL после явного Confirm; реальных платежей/сетевых покупок при проверке не выполняли. Визуальная примерка отсутствует |
| `Notes` | `notes` с optional tag | Вывод текстом; исходный GPUI layout и rich media отсутствуют |
| `Settings` | `settings`, `game-args`, `no-ai`, `native-cache`, `rust`, `java`, `data`, `links`; `legacy storage-status/move/retry/default` | Перенос запускает существующий relocation helper после выхода окна; Win7 использует Toolhelp32 вместо PowerShell 3. Appearance/accent/background/opening сохраняются для modern UI |

| Вкладка Home / действие | Native путь | Доказательство и оставшийся тест |
|---|---|---|
| Overview, версия, аргументы | `versions --installed`, `settings`, `game-args`, `instance version`, `instance launch`, `play` | Parser/core tests; полный Win7 сценарий не проверен |
| Создание/выбор/редактирование экземпляров | `instance list/create/select/rename/group/icon/version/java/loader/launch/shortcut/delete` | Unicode/flags/repeated/default argument contract tests и существующие instance tests; Win7 UI/filesystem smoke ещё нужен |
| Импорт/экспорт экземпляров | `instance import/export/others/copy-from`; `import-build` | Существующий core и его проверки используются; drag-and-drop и native file picker отсутствуют, путь вводится в форме |
| Mods | `mods` и `native-build` | Content actions доступны; Win7 native build возвращает явную диагностическую ошибку |
| Worlds | `worlds list/backup/copy/delete`; `play --world` | Существующие worlds tests; тест реальных пользовательских миров на Win7 не проводился |
| Datapacks мира | `worlds datapack-list/datapack-add/datapack-remove`; browser install с `--world` | Существующий core; Win7 операция/пути/корзина не проверены |
| Multiplayer list | `multiplayer list/add/remove`; `play --server` | Существующий `server_list`; присоединение к игре не проверено |
| Resource/shader packs | `packs list/add/on/off/remove`; `mods search/install` | Contract test выбора pack/flags и существующие pack tests; GPU shader execution не является функцией лаунчера |
| Screenshots | `screenshots list/delete` | Списки/удаление через core; native просмотр изображений/thumbnail gallery отсутствует |
| Console, прогресс, ошибки | `logs`, `share-log`, `unshare-log`; stdout/stderr worker в native output control | Буфер ограничен по строкам и байтам, oversized line не выделяется целиком; GUI console input/real game lifecycle не прошли приёмку |
| Аккаунты | Native SignIn panel и `account` формы | URL/код/status/copy предусмотрены; sign-in/refresh/ownership не выполнены без Azure client ID и реального аккаунта |
| Установка ресурсов | `prepare --download` либо `prepare --minecraft PATH` | Existing SHA/ownership пути сохранены; самостоятельная чистая Win7 установка с Java 25 ещё не доказана |

Каталог строит все видимые leaf-команды CLI, кроме `mcp`, `completions`, `agent-config`,
`benchmark`, `account session` и `account sign-in`. Последняя обслуживается специальной
панелью, а `account session` не выводится формой, поскольку это путь к секретному session
file. CLI остаётся доступен тем же executable. Доступность `mcp` через CLI не означает,
что native GUI-only операции появились в MCP.

## Сборка и артефакты

Воспроизводимый путь описан в [packaging/win7/TESTING.md](../packaging/win7/TESTING.md):

```sh
bash tools/bootstrap_win7_cross.sh
bash tools/build_win7_launcher.sh
```

Скрипт использует `nightly-2026-11-01`, target `x86_64-win7-windows-gnu`,
`-Z build-std=std,panic_abort`, `-C target-cpu=x86-64`, static CRT и subsystem 6.1.
Сборка основного workspace не понижается на старый Rust. Bootstrap распаковывает
SHA-256-pinned MinGW packages в локальный build-каталог; Windows-пользователю эти tools
не нужны. Старому `windows-targets 0.48.5` отдельно передаётся путь его Cargo-verified
import library, поскольку его build script не распознаёт vendor `win7`.

Ожидаемые выходы успешного полного скрипта:

- `horizon-launcher-win7-x64.zip`: native launcher, инструкция, build provenance и PE report;
- `horizon-win7-process-test-x64.zip`: отдельный mock client для проверки process contract;
- `imports-win7-x64.json`, `imports-mock-win7-x64.json`, `build.txt`, `SHA256SUMS.local`.

`SHA256SUMS.local` — контрольные суммы локальной сборки, **не подпись релиза**.
В ZIP не включаются Mojang assets, game jars, подготовленные ресурсы и чужие аккаунты.
Основной download/release source и доверенные ключи Horizon сами по себе не создают
подписанный Win7 release в пользовательском форке: публикация, источник и доверенная
подпись остаются отдельной обязательной частью поставки. Для отдельной legacy сборки
предусмотрены build-time `HORIZON_WIN7_RELEASE_REPO=owner/repository` и
`HORIZON_WIN7_RELEASE_PUBLIC_KEYS` (base64 Ed25519, по одному ключу на строку).
Произвольный fork без корректного публичного ключа не получает разрешения устанавливать
unsigned releases. Modern builds продолжают использовать прежние repository/keys.

## Проверки и границы доказательств

Получены следующие результаты (это не аппаратная приёмка Win7):

- `cargo fmt --manifest-path launcher/Cargo.toml` и релевантный rustfmt: PASS.
- Core-only library suite: **653 passed, 7 ignored**. Native entrypoint suite: **35 passed**.
- После точечных изменений маршрутизации runtime: `java::` **9 passed**,
  `rust_toolchain::` **17 passed**; Clippy core + native binary с `-D warnings`: PASS.
- Подготовка Java: **5 passed**; DNS resolver после отключения неиспользуемой feature:
  **7 passed**; PE audit tests: **10 passed**; полнота переводов: PASS.
- `x86_64-win7-windows-gnu`, nightly-2026-11-01, `build-std`, release: собраны
  launcher и отдельный mock. Статический PE audit обоих EXE: PASS, subsystem 6.1.
  Legacy dependency graph не содержит GPUI/wgpu runtime; D3D/DXGI/Vulkan/WinRT/combase
  imports отсутствуют. Динамические вызовы и инструкции CPU всё равно требуют проверки.
- Wine 10 с версией 6.1.7601 / Xvfb 1024×600: окно, mouse/Tab/scroll, Versions worker,
  создание и запись экземпляра `Тест Кириллица 1`, завершение worker и восстановление кнопок:
  PASS. Без client ID Microsoft button вернула безопасный отказ.
- Mock с кириллицей/пробелами проверил resources/home/cwd/inherited env и child renderer:
  PASS. Это не игровой запуск и не проверка сессии реального владельца.
- Liberica Java 25 `-Xmx512m -version` и managed helper с заранее скачанным официальным
  SHA-проверенным архивом в ASCII каталоге: PASS под Wine. Сеть при этом не проверялась.
  Обнаружен отказ JVM bootstrap в кириллическом каталоге; runtime перенесён в уже имеющийся
  ASCII tool directory core. Повторный тест исправления и его точный SHA отражаются в
  прилагаемом smoke-протоколе, если выполнены.

Все **108 native форм** перечислены по отдельности в
[win7-action-parity.md](win7-action-parity.md), с отдельными статусами обеих Win7 GPU-систем.
`horizon-launcher-win7 --list-native-actions` выдаёт точное описание полей из текущего CLI.
Подписанных публичных legacy-релизов пока нет; `SHA256SUMS.local` — контрольные суммы
локальной экспериментальной сборки, а не подпись релиза. Точные hashes входят в артефакт.

| Проверка | Что проверяет | Чего не доказывает |
|---|---|---|
| `cargo check --lib --no-default-features` и вариант с `win7` | Core компилируется без обязательного GUI | Запуск EXE на Win7 |
| `cargo tree --target x86_64-win7-windows-gnu --no-default-features --features win7` | Отсутствие GPUI/wgpu runtime в legacy graph | Отсутствие всех новых Windows imports в итоговом EXE |
| Catalog/worker/backend/Java tests | Аргументы без shell quoting, Unicode, отмена signed transfer, hash identity, staging/rollback/recovery, границы output, сохранение предыдущего runtime | Реальный Microsoft login, exporter run и поведение Windows Shell на Win7 |
| `tools/check_win7_imports.py` и Python tests | Архитектура/subsystem, normal и delay imports; неизвестные DLL/API отвергаются | Динамические `LoadLibrary/GetProcAddress`, CPU dispatch, все DLL runtime, железо |
| `--smoke-client` с отдельным mock | Реальная граница процесса, `--resources`, home/cwd, env inheritance, child legacy graphics selector | Авторизованный игровой запуск и работоспособность ТЗ №2 |
| Wine/Xvfb, если выполнено | Дополнительный запуск Windows binary вне Windows | Windows 7 SP1, драйвер Intel/AMD, Aero-off и производительность Atom |

При отделении GUI выявлен скрытый effective feature: GPUI включал
`serde_json/preserve_order`. Core-only build сохраняет эту feature явно, чтобы не менять
порядок JSON objects, используемый существующим Forge installation planner.

### Аппаратная матрица

| Цель | Native GUI / ввод / DPI / Aero-off | Mock process contract | Реальный клиент | Лицензионный fresh install | Startup/RSS/peak memory |
|---|---|---|---|---|---|
| Win7 SP1 x64, Atom N455, GMA 3150, 2 ГБ, Intel driver | НЕ ПРОВЕРЕНО | НЕ ПРОВЕРЕНО на Win7 | НЕ ПРОВЕРЕНО | НЕ ПРОВЕРЕНО | НЕ ИЗМЕРЕНО |
| Win7 SP1 x64, Radeon HD 4570, AMD driver | НЕ ПРОВЕРЕНО | НЕ ПРОВЕРЕНО на Win7 | НЕ ПРОВЕРЕНО | НЕ ПРОВЕРЕНО | НЕ ИЗМЕРЕНО |
| Win7 SP1 x86 | Не собрана, дополнительная цель исключена | — | — | — | — |
| Современный GPUI launcher на Win10/11/Linux/macOS | Исходные UI сохранены; полный regression runtime ещё нужен | — | Не относится к mock | Не подтверждено этим legacy-прогоном | Не измерено |

### Точные ограничения

- **L3 не принят.** Установка pinned Liberica Java 25 — кандидат на автономный Win7 путь,
  а не доказанная поддержка runtime. Детали: [win7-prepare-audit.md](win7-prepare-audit.md).
  Нужны `java`/`javac` smoke на настоящей Win7 и полный exporter run на собственных
  лицензионных данных без современного ПК. Ограничения 512 MiB/один job не доказывают
  успешную подготовку всех данных.
- **Microsoft вход не принят.** Пользователь сообщил, что Azure public client ID пока
  отсутствует. Сборка должна сохранять существующий безопасный отказ и ownership gate,
  пока не настроен `HORIZON_MSA_CLIENT_ID`. Токены в command line не добавляются.
- **Native mods не завершены.** Текущий механизм пересобирает весь клиент, а не загружает
  готовую совместимую DLL. Нужны Win7 target, корректный ABI, отдельный cache/toolchain и
  проверка локальной либо явно реализованной делегированной сборки. Кнопка не скрывается.
- **Отмена частичная.** Device Code отменяется через существующий token; signed archive
  downloads проверяют cancel file до и между transfer/commit шагами. Browser/modpack
  bespoke downloads и exporter loops не имеют полной сквозной отмены. UI ожидает безопасной
  границы; принудительное убийство установки/сохранения мира не используется.
- **Resume частичный.** Повторная попытка и уже проверенный Java archive сохраняются при
  ошибке extraction/probe. Возобновление transfer произвольного файла с последнего байта
  через HTTP Range не заявляется реализованным.
- **Пути частично ограничены.** Unicode передаётся без shell, однако Win7 recycle fallback
  отказывает extended/слишком длинным и UNC путям, не удаляя их. Полная приёмка длинных
  путей installer/update/toolchain остаётся невыполненной.
- **L4 частичный.** Есть отдельный ZIP/build pipeline и recovery code; нет подтверждённой
  публичной подписанной legacy поставки и реального Win7 обновления. Native graphical
  installer wizard не реализован. Native entrypoint не вызывает общий quiet installer,
  чтобы не перезаписать регистрацию современного launcher; поставка пока только ZIP.
- **L2 не принят аппаратно.** 108 форм дают доступ к существующим операциям, включая
  wardrobe/shop/storage и console input. GPUI 3D previews, rich gallery и drag-and-drop
  не перенесены; пути вводятся в native полях. Сетевые сценарии и полный паритет на Win7
  требуют реального аккаунта и совместимого клиента.

## Этапы и следующая приёмка

| Этап | Состояние | Что осталось |
|---|---|---|
| L0 | Аудит и статический PE audit обоих EXE выполнены | Проверка поздних API и аппаратный запуск |
| L1 | Native окно и process probe прошли Wine smoke | Реальная Win7 проверка обеих GPU-систем |
| L2 | Общий core доступен через native forms, есть специальный вход/логи | Закрыть отмеченные GUI gaps, проверить каждое действие на Win7 |
| L3 | Добавлен pinned JVM candidate и экономный explicit профиль подготовки | Доказать runtime и автономный fresh install с реальным владельцем |
| L4 | Реализованы отдельная упаковка и legacy update policy/recovery | Подписанные releases, источник доверия, interrupted update/rollback на Win7 |
| L5 | Не выполнен | GMA 3150/N455/2 ГБ, HD 4570 и regression modern launcher |

Порядок пользовательской проверки и команды mock приведены в `packaging/win7/TESTING.md`.
Для обоих GPU нужно записать build7601, установленные KB, версию драйвера, права запуска,
размер экрана/DPI, состояние Aero, startup time, idle CPU, RSS и peak memory.
Каждое действие выше требует собственного PASS/FAIL/NOT TESTED с логом. Проверки неверного
хеша/подписи, нет сети, отмены, нехватки диска, Unicode/длинного пути, неверного аккаунта и
обновления после принудительного завершения остаются обязательными.

## Изменённые файлы и назначение

| Точный путь | Обязательная функция / причина |
|---|---|
| `launcher/Cargo.toml` | Изолировать modern GUI и Win7 binary; не компилировать GPU framework; сохранить прежний JSON order |
| `launcher/src/bin/horizon-launcher-win7.rs` | Отдельный startup/CLI/native entrypoint, recovery и диагностика без GPUI |
| `launcher/src/bin/mock-horizon-client.rs` | Отдельный process-contract fixture; не игровой клиент и не owner bypass |
| `launcher/src/win7/mod.rs` | Граница native UI модулей |
| `launcher/src/win7/catalog.rs` | Формы из существующего CLI, flags/defaults/repeated args/Unicode и parser validation |
| `launcher/src/win7/window.rs` | USER32/GDI окно, поля, фокус/scroll, Device Code, clipboard, status/output |
| `launcher/src/win7/worker.rs` | Фоновые CLI workers, cooperative cancel, ограниченный журнал, профиль памяти подготовки |
| `launcher/src/win7/smoke.rs` | Изолированный запуск mock через общий low-level launch contract |
| `launcher/src/backend.rs` | Отдельные legacy assets/маркер SHA, launch env, совместимость, staging/rollback/recovery, cancel и подготовка Java |
| `launcher/src/signed.rs` | Использовать выбранный при сборке legacy release source, сохраняя обязательную проверку подписи |
| `launcher/src/backend/win7_tests.rs` | Контракт selection/identity/launch/update/recovery legacy без подмены реальным игровым запуском |
| `launcher/src/cli.rs` | Прогресс pipe-worker и Win7-only extension над тем же core |
| `launcher/src/cli_tools.rs` | Не включать checkout/storage extension в agent tools; modern command reference не изменяется |
| `launcher/src/win7_actions.rs` | Wardrobe/shop/storage формы через существующие core API, Confirm и migration handoff |
| `launcher/src/java.rs` | Java 25 manual install и discovery используют тот же legacy runtime |
| `launcher/src/rust_toolchain.rs` | Отказ несовместимому современному Rust host до его загрузки на Win7 |
| `launcher/src/relocation.rs`, `launcher/src/relocation/win7.rs`, `launcher/src/relocation/tests.rs` | Toolhelp32 guard вместо PowerShell 3 в Win7 migration и тест блокировки активных процессов |
| `launcher/src/lib.rs` | Подключение изолированных Win7 helpers |
| `launcher/src/legacy_java.rs` | Pinned official JVM download, SHA-256, layout/version probe, staging и rollback |
| `launcher/src/native_mods.rs` | Явный отказ от несовместимой native сборки вместо скрытой modern установки |
| `launcher/src/pictures.rs` | Исключение GPU probe из core-only сборки |
| `launcher/src/win7_shell.rs` | Совместимые shell links/корзина без зависимости от WinRT wrappers |
| `launcher/src/shortcut.rs` | Перенаправление legacy ShellLink в Win7 helper |
| `launcher/src/desktop_seams.rs` | Использование совместимой Win7 корзины с существующим core |
| `launcher/src/installer.rs` | Проверка shell shortcut также для core-only Windows target |
| `crates/horizon-prepare/src/java.rs` | Explicit heap cap только при opt-in env, без изменения modern default |
| `crates/horizon-prepare/src/worker.rs` | Применение heap cap к exporter JVM workers |
| `crates/horizon-protocol/Cargo.toml` | Отключить неиспользуемую `ipconfig/computer` feature, тянувшую несовместимые Windows Registry/WinRT imports; алгоритм adapter DNS не меняется |
| `Cargo.lock`, `launcher/Cargo.lock` | Зафиксировать минимальное изменение dependency graph, необходимое для Win7 ABI |
| `tools/bootstrap_win7_cross.sh` | Воспроизводимые pinned cross tools в локальном каталоге |
| `tools/build_win7_launcher.sh` | Отдельные target/std/CPU flags, no-GPU graph check, PE gate и ZIPs |
| `tools/check_win7_imports.py` | Строгая статическая проверка Win7 PE normal/delay imports |
| `tests/test_check_win7_imports.py` | Контракт проверки PE и отказ новых/неизвестных импортов |
| `.github/workflows/launcher-win7.yml` | Отдельная experimental CI сборка без публикации как готового релиза |
| `packaging/win7/TESTING.md` | Воспроизводимая сборка, mock и точная приёмка на целевом железе |
| `packaging/win7/API_BASELINE.md` | Обоснование Win7-or-earlier API, включённых в строгий PE import allowlist |
| `docs/win7-prepare-audit.md` | Доказательства Java blocker/candidate и оставшиеся exporter проверки |
| `docs/win7-launcher-status.md` | Этот отчёт и неполные критерии; основной README не затронут |
| `docs/win7-action-parity.md` | Отдельная строка и статус каждого native действия |
| `launcher/lang/*.json` — точные имена ниже | Новые сообщения native UI/диагностики во всех существующих каталогах; там, где перевод ещё не готов, приняты существующие English fallbacks |

Точные изменённые каталоги переводов (одна и та же причина изменения каждого файла):

```text
launcher/lang/af.json
launcher/lang/ar.json
launcher/lang/az.json
launcher/lang/ba.json
launcher/lang/be.json
launcher/lang/bg.json
launcher/lang/br.json
launcher/lang/bs.json
launcher/lang/ca.json
launcher/lang/cs.json
launcher/lang/cv.json
launcher/lang/cy.json
launcher/lang/da.json
launcher/lang/de.json
launcher/lang/el.json
launcher/lang/eo.json
launcher/lang/es.json
launcher/lang/et.json
launcher/lang/eu.json
launcher/lang/fa.json
launcher/lang/fi.json
launcher/lang/fil.json
launcher/lang/fo.json
launcher/lang/fr.json
launcher/lang/fur.json
launcher/lang/fy.json
launcher/lang/ga.json
launcher/lang/gd.json
launcher/lang/gl.json
launcher/lang/haw.json
launcher/lang/he.json
launcher/lang/hi.json
launcher/lang/hr.json
launcher/lang/hu.json
launcher/lang/hy.json
launcher/lang/id.json
launcher/lang/ig.json
launcher/lang/is.json
launcher/lang/it.json
launcher/lang/ja.json
launcher/lang/ka.json
launcher/lang/kk.json
launcher/lang/kn.json
launcher/lang/ko.json
launcher/lang/ky.json
launcher/lang/la.json
launcher/lang/lb.json
launcher/lang/li.json
launcher/lang/lmo.json
launcher/lang/lo.json
launcher/lang/lt.json
launcher/lang/lv.json
launcher/lang/mk.json
launcher/lang/mn.json
launcher/lang/ms.json
launcher/lang/mt.json
launcher/lang/nl.json
launcher/lang/no.json
launcher/lang/oc.json
launcher/lang/pl.json
launcher/lang/pt-br.json
launcher/lang/pt.json
launcher/lang/ro.json
launcher/lang/ru.json
launcher/lang/sah.json
launcher/lang/se.json
launcher/lang/sk.json
launcher/lang/sl.json
launcher/lang/so.json
launcher/lang/sq.json
launcher/lang/sr.json
launcher/lang/sv.json
launcher/lang/szl.json
launcher/lang/ta.json
launcher/lang/th.json
launcher/lang/tr.json
launcher/lang/tt.json
launcher/lang/uk.json
launcher/lang/ur.json
launcher/lang/uz.json
launcher/lang/vec.json
launcher/lang/vi.json
launcher/lang/yi.json
launcher/lang/yo.json
launcher/lang/zh.json
launcher/lang/zu.json
```
