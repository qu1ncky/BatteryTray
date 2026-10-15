# BatteryTray

Заряд беспроводных наушников, мышей, клавиатур и геймпадов в трее Windows — и напоминание, которое не даст им сесть внезапно.

*English: a Windows tray app that shows the battery level of wireless headsets, mice, keyboards and gamepads (Razer Barracuda X via its dongle, Attack Shark / Rongyuan 2.4 GHz mice, SteelSeries mice, any Bluetooth device Windows knows the battery of) and pops up a reminder that stays until you act. Download the zip from Releases and run `install.cmd`.*

<img src="docs/screenshot.png" alt="Окно BatteryTray" width="420">

## Что умеет

- **Значок в трее** — цифра заряда устройства, которое сядет первым. Зелёная; жёлтая ниже порога напоминания; красная ниже критического; синяя при зарядке. Наведите курсор — увидите все устройства.
- **Окно со всеми устройствами** — клик по значку. Подключённые — карточками с процентом, выключенные — списком с последним известным зарядом и временем.
- **Напоминание о разряде** — своё окно поверх всех окон в углу экрана, а не системное уведомление: его не глушит «Не беспокоить», оно не пропадает само и не забирает фокус у игры. Кнопки «Напомнить через N мин» и «Не напоминать до зарядки». На критическом уровне появится ещё раз, даже если его отложили.
- **Настройки** — порог напоминания, критический уровень, интервал повтора, звук; для каждого устройства — своё название, скрыть, отключить напоминания.
- **Обновляется сама** с GitHub: новая версия ставится тихо, пока окна программы закрыты, а потом всплывает «BatteryTray обновлён».
- Запускается при входе в Windows, есть ярлык в «Пуске». Ничего не меняет в устройствах: только читает статус.

## Какие устройства поддерживаются

| Устройство | Подключение | Статус |
|---|---|---|
| Razer Barracuda X (2022) | USB-C донгл 2.4 ГГц (`1532:0550`) | ✅ проверено |
| Razer Barracuda X (2022), вариант донгла `1532:0552` | 2.4 ГГц | ⚠️ не проверено |
| Attack Shark R3 | приёмник 2.4 ГГц (`3151:402D`) | ✅ проверено |
| Другие мыши и клавиатуры на платформе Rongyuan (Attack Shark и др., 81 модель приёмника) | приёмник 2.4 ГГц | ⚠️ не проверено |
| SteelSeries Aerox 3 / 5 / 9 Wireless, Prime Wireless, Prime Mini Wireless, Rival 3 Wireless (и Gen 2), Rival 650 Wireless | приёмник 2.4 ГГц или кабель | ⚠️ не проверено |
| Мыши Razer: Viper Ultimate / V2 Pro / V3 Pro / Mini SE, DeathAdder V2 Pro / V3 Pro / V3 HyperSpeed / V4 Pro, Basilisk Ultimate / V3 Pro / V3 Pro 35K / X HyperSpeed, Naga Pro / V2 Pro, Cobra Pro / HyperSpeed, Pro Click (и Mini, V2), Lancehead, Mamba, Orochi V2, Atheris и др. — около 40 моделей | приёмник HyperSpeed или кабель | ⚠️ не проверено |
| Клавиатуры Razer: BlackWidow V3 Pro / V3 Mini / V4 Mini / V4 TKL HyperSpeed, DeathStalker V2 Pro (и TKL), Joro | приёмник или кабель | ⚠️ не проверено |
| Мыши, клавиатуры и другие устройства Logitech с протоколом HID++ (MX Master / Anywhere, MX Keys, серия G Lightspeed, M- и K-серии и др.) | приёмник Unifying / Bolt / Lightspeed / Nano или кабель | ⚠️ не проверено |
| Гарнитуры HyperX: Cloud Alpha Wireless, Cloud II Wireless, Cloud Flight | приёмник | ⚠️ не проверено |
| Гарнитуры Logitech G: G533, G535, G633 / G635 / G733 / G933 / G935, G PRO / PRO X Wireless, G930, ASTRO A50 (Gen 5) | приёмник | ⚠️ не проверено |
| Гарнитуры SteelSeries Arctis: 1, 7, 9, Pro Wireless, 7+, Nova 3P, Nova 5, Nova 7 (и 7X, 7P, Gen 2), Nova Pro Wireless, GameBuds | приёмник | ⚠️ не проверено |
| Гарнитуры Corsair: Void (вся серия), HS70, Void / Virtuoso / HS80 MAX Wireless, Virtuoso XT / SE | приёмник | ⚠️ не проверено |
| Sony INZONE H5, MCHOSE X9, Lenovo Wireless VoIP | приёмник | ⚠️ не проверено |
| Sony DualSense, DualSense Edge, DualShock 4 | кабель, адаптер Sony; Bluetooth — если игра или Steam включили полный режим геймпада | ⚠️ не проверено |
| Геймпады Xbox через адаптер Xbox Wireless (XInput) | адаптер | ⚠️ не проверено; XInput знает только 4 уровня заряда |
| Любое Bluetooth-устройство, заряд которого показывает сама Windows (геймпад Xbox, многие мыши, клавиатуры, гарнитуры) | Bluetooth | ✅ проверено (геймпад Xbox) |
| Любое USB-устройство, которое сообщает заряд стандартным полем HID «Battery Strength» | приёмник или кабель | ⚠️ не проверено |

Bluetooth-устройства показываются, только пока подключены: после отключения Windows хранит устаревший процент.

Процент у Razer Barracuda X — оценка по напряжению батареи, шагами по 5%: другого наушники через донгл не сообщают. Во время зарядки напряжение завышено, поэтому показывается уровень до зарядки.

## Установка

1. Скачайте `BatteryTray-…-win-x64.zip` со страницы [Releases](https://github.com/qu1ncky/BatteryTray/releases/latest) и распакуйте.
2. Запустите `install.cmd` и подтвердите запрос администратора.
3. Значок появится у часов. Windows 11 прячет новые значки под стрелку **^** — перетащите его на панель задач, чтобы цифра была видна всегда.

Программа ставится в `C:\Program Files\BatteryTray` и запускается при входе в Windows задачей Планировщика «BatteryTray». .NET устанавливать не нужно.

Обновлять вручную не нужно — см. [Обновления](#обновления). Версию 1.0.0 обновлений ещё не умела: с неё один раз поставьте новый релиз через `install.cmd`. Удаление — `uninstall.cmd` из архива.

**Windows может предупредить**, что приложение неизвестное: exe не подписан сертификатом. Нажмите «Подробнее» → «Выполнить в любом случае».

**Без установки.** Можно просто запустить `BatteryTray.exe` откуда угодно: всё работает, а автозапуск включается в «Настройки → Система» (с обычными правами).

### Зачем права администратора

На некоторых компьютерах сторонние драйверы закрывают доступ к служебному каналу USB-приёмника. Например, драйвер G-SPY (`vkbms.inf`) от некоторых дешёвых клавиатур забирает себе все HID-устройства с usage `FF00:0001` — среди них донгл Razer — и открывает их только администраторам. Тогда у устройства написано «нужны права администратора». Поэтому установленная программа запускается задачей с наивысшими правами (без окна UAC при каждом входе), а лежит в Program Files, где её exe не подменить без прав администратора.

## Обновления

Через полминуты после запуска и потом раз в 6 часов программа смотрит последний релиз этого репозитория. Если он новее:

- **с автообновлением** (по умолчанию) — скачивает `BatteryTray.exe` из релиза, проверяет его SHA-256 по контрольной сумме, которую публикует GitHub, и версию внутри файла, подменяет свой exe и перезапускается. Делает это, только пока окна программы закрыты, чтобы не пропасть у вас из-под курсора;
- **без автообновления** или если для записи в Program Files нужны права администратора (обычная учётная запись) — один раз сообщает о новой версии, а ставится она кнопкой «Обновить» (в последнем случае — с подтверждением UAC).

Всё это — в «Настройки → Обновления»: статус, что нового, «Проверить сейчас», переключатель автообновления. Отключённое автообновление не мешает ставить версии кнопкой.

Скачанный файл кладётся сразу в папку программы (в Program Files писать могут только администраторы), так что подменить его по дороге другой программе нельзя. Старый exe остаётся рядом как `BatteryTray.old.exe` и удаляется при следующем запуске.

## Моего устройства нет

Выполните в PowerShell, пока устройство подключено, и пришлите результат в [Issues](https://github.com/qu1ncky/BatteryTray/issues):

```powershell
Get-PnpDevice -PresentOnly | ? InstanceId -match '^USB\\VID_\w{4}&PID_\w{4}\\' | % { "{0}  {1}" -f $_.InstanceId.Substring(4,17), (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_BusReportedDeviceDesc).Data }
```

Пригодится и диагностика: `"C:\Program Files\BatteryTray\BatteryTray.exe" --dump "%USERPROFILE%\Desktop\battery.txt"` — один опрос всех источников в файл, и лог `%LOCALAPPDATA%\BatteryTray\log.txt`.

Новое устройство добавляется отдельным классом в `Providers/` (интерфейс `IBatteryProvider`: опросить устройство и сказать, когда опрашивать снова).

## Как это работает

| Источник | Протокол |
|---|---|
| `RazerBarracudaProvider` | Команды Airoha RACE через донгл: перевести донгл в remote-режим, запросить напряжение (`GET_BATTERY`), вернуть local-режим. Напряжение → процент по кривой Li-ion; зарядка определяется по разбросу напряжения между замерами. Порт из [razer-barracuda-x-battery](https://github.com/mehdibouchami/razer-barracuda-x-battery). |
| `RongyuanDongleProvider` | Feature-отчёт `[00 F7 …]` на интерфейсе 2 (vendor page `FFFF`), ответ `[00 canRead kbBattery mouseBattery kbOffline mouseOffline …]`. Найден в официальном веб-драйвере ATTACK SHARK, который шлёт эту команду каждые 5 секунд. |
| `SteelSeriesProvider` | Output-отчёт `0x92` / `0xD2` (Aerox, Prime) или `AA 01` (Rival) на служебном интерфейсе, по [rivalcfg](https://github.com/flozz/rivalcfg). |
| `RazerPeripheralProvider` | 90-байтный feature-отчёт OpenRazer: команда `07 80` (заряд 0–255), `07 84` (зарядка), CRC — XOR байтов 2–87; transaction id и пауза между запросом и ответом — свои у каждой модели, по таблице OpenRazer. |
| `LogitechProvider` | HID++: пинг слотов 1–6 приёмника (или индекса `0xFF` у устройства на кабеле), поиск фичи заряда `0x1004` / `0x1000` / `0x1001` / `0x1F20` и имени `0x0005`; для старых устройств HID++ 1.0 — регистры `0x0D` / `0x07`. Короткие запросы пишутся в короткую коллекцию, ответы читаются из обеих — Windows делит интерфейс на две. По [Solaar](https://github.com/pwr-Solaar/Solaar). |
| `HeadsetProvider` | Запрос и ответ на служебном HID-канале гарнитуры, байты — по [HeadsetControl](https://github.com/Sapd/HeadsetControl) (своя реализация). У гарнитур Logitech G ответ — напряжение, процент считается по таблицам. |
| `StandardHidBatteryProvider` | Ищет у всех USB HID-устройств поле Generic Device Controls / Battery Strength (`06:20`) в feature-отчёте и читает его. |
| `PlayStationProvider` | Ничего не отправляет: читает один входной отчёт геймпада, байт статуса (DualSense — `data[53]` по USB / `data[54]` по Bluetooth, DualShock 4 — `data[30]` / `data[32]`), как драйвер Linux `hid-playstation`. |
| `XInputProvider` | `XInputGetBatteryInformation`: 4 уровня → примерный процент. Геймпады по Bluetooth пропускает — у них точный процент даёт Windows. |
| `BluetoothProvider` | Свойство устройства `DEVPKEY_Bluetooth_Battery` (то, что показывает «Параметры → Bluetooth»), только при `ConnectionStatus = Connected`. |

Файлы: `%LOCALAPPDATA%\BatteryTray` — `settings.json` (пороги, названия, скрытые устройства), `state.json` (последний известный заряд), `log.txt` (подключения, изменения заряда, напоминания).

## Сборка из исходников

Нужен [.NET 8 SDK](https://dotnet.microsoft.com/download/dotnet/8.0).

```
dotnet build                      # отладочная сборка
install.cmd                       # собрать и установить
powershell -File build-release.ps1  # exe и zip для релиза в dist\
```

### Как выпустить новую версию

1. Поднимите `<Version>` в `BatteryTray.csproj` (например, `1.2.0`).
2. `powershell -File build-release.ps1`.
3. `gh release create v1.2.0 dist\BatteryTray-1.2.0-win-x64.zip dist\BatteryTray.exe --title "BatteryTray 1.2.0" --notes "что нового"`.

Автообновление берёт из последнего релиза файл именно с именем `BatteryTray.exe`, а номер в теге должен совпадать с версией внутри exe — иначе обновление откажется ставиться. Текст релиза программа показывает в «Настройках» как «что нового». Черновики и пре-релизы автообновление не видит — ими удобно проверять сборку до выпуска.

## Лицензия

MIT — см. [LICENSE](LICENSE). Чужой код и источники протоколов — в [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Проект не связан с Razer, SteelSeries, ATTACK SHARK и Microsoft; названия — товарные знаки их владельцев.
