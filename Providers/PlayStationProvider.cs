using System;
using System.Collections.Generic;
using System.Linq;
using BatteryTray.Core;
using BatteryTray.Hid;

namespace BatteryTray.Providers
{
    /// <summary>
    /// Sony DualSense and DualShock 4. They stream their state as input reports, the battery included, so nothing
    /// is sent - one report is read. Layout from the Linux driver (drivers/hid/hid-playstation.c):
    ///   DualSense: status byte = data[53] (USB report 0x01) or data[54] (Bluetooth report 0x31);
    ///     low nibble 0-10 = level in tens, high nibble 0 = discharging, 1 = charging, 2 = full.
    ///   DualShock 4: status bytes = data[30..31] (USB report 0x01) or data[32..33] (Bluetooth report 0x11);
    ///     low nibble = level, bit 4 = cable plugged in (then 0-10 = charging, 11 = full); wireless adapter:
    ///     bit 2 of the second byte = no controller connected.
    /// Over Bluetooth the controller sends the short report 0x01 without the battery until a game or Steam switches
    /// it to the full report; switching it from here could confuse a running game, so that is left alone.
    /// </summary>
    public sealed class PlayStationProvider : IBatteryProvider
    {
        const ushort SonyVid = 0x054C;
        const int TimeoutMs = 250;
        const int NoController = -1, BatteryError = -2;

        static readonly Dictionary<ushort, (string Name, bool DualSense)> Models = new()
        {
            [0x0CE6] = ("DualSense", true),
            [0x0DF2] = ("DualSense Edge", true),
            [0x05C4] = ("DualShock 4", false),
            [0x09CC] = ("DualShock 4", false),
            [0x0BA0] = ("DualShock 4", false), // Sony wireless adapter
        };

        public string Name => "PlayStation";

        public IReadOnlyList<DeviceReading> Poll()
        {
            var result = new List<DeviceReading>();
            var pads = HidBus.Enumerate((v, p) => v == SonyVid && Models.ContainsKey(p))
                .Where(i => i.AccessDenied || (i.UsagePage == 0x01 && i.Usage == 0x05 && i.InputLength > 0))
                .ToList();
            foreach (HidInfo info in pads)
            {
                var (name, dualSense) = Models[info.ProductId];
                bool bluetooth = info.Path.Contains("{00001124-", StringComparison.OrdinalIgnoreCase);
                string via = bluetooth ? "Bluetooth" : info.ProductId == 0x0BA0 ? "адаптер" : "кабель";
                // Several identical controllers get #2, #3... in the order Windows lists them
                string id = $"ps:{info.ProductId:x4}:{(bluetooth ? "bt" : "usb")}";
                int same = result.Count(r => r.Id.StartsWith(id, StringComparison.Ordinal));
                if (same > 0)
                {
                    id += "#" + (same + 1);
                    name += " #" + (same + 1);
                }

                DeviceReading reading = new()
                {
                    Id = id, Name = name, Kind = DeviceKind.Gamepad, Via = via, Online = false,
                    Detail = info.AccessDenied ? "нет доступа (устройство скрыто другой программой)" : "нет связи",
                };
                if (!info.AccessDenied)
                {
                    try
                    {
                        reading = Read(info, dualSense, reading);
                    }
                    catch (Exception e)
                    {
                        reading = reading with { Detail = "ошибка: " + e.Message };
                    }
                }
                result.Add(reading);
            }
            return result;
        }

        public TimeSpan Delay(IReadOnlyList<DeviceReading> last)
        {
            return TimeSpan.FromSeconds(60);
        }

        static DeviceReading Read(HidInfo info, bool dualSense, DeviceReading template)
        {
            using var dev = new HidDevice(info, readOnly: true);
            dev.Flush();
            DateTime deadline = DateTime.UtcNow.AddMilliseconds(TimeoutMs);
            bool any = false;
            while (DateTime.UtcNow < deadline)
            {
                byte[]? r = dev.Read(Math.Max((int)(deadline - DateTime.UtcNow).TotalMilliseconds, 1));
                if (r == null) break;
                any = true;
                var battery = dualSense ? ParseDualSense(r, template.Via == "Bluetooth") : ParseDualShock4(r, template.Via);
                if (battery == null) continue;
                if (battery.Value.Level == NoController) return template with { Detail = "контроллер не подключён к адаптеру" };
                if (battery.Value.Level == BatteryError)
                    return template with { Online = true, Detail = "контроллер сообщает об ошибке батареи или перегреве" };
                return template with { Online = true, Level = battery.Value.Level, Charging = battery.Value.Charging, Detail = null };
            }
            // Reports arrive, but none carries the battery: the short Bluetooth report
            return any
                ? template with { Online = true, Detail = "заряд появится, когда игра или Steam включат полный режим геймпада" }
                : template;
        }

        static (int Level, bool Charging)? ParseDualSense(byte[] r, bool bluetooth)
        {
            int at = bluetooth ? 54 : 53;
            if (r.Length <= at || r[0] != (bluetooth ? 0x31 : 0x01)) return null;
            int data = r[at] & 0x0F, status = r[at] >> 4;
            return status switch
            {
                0 => (Math.Min(data * 10 + 5, 100), false),
                1 => (Math.Min(data * 10 + 5, 100), true),
                2 => (100, true),
                _ => (BatteryError, false), // 0xA/0xB voltage or temperature error, 0xF charging error
            };
        }

        static (int Level, bool Charging)? ParseDualShock4(byte[] r, string via)
        {
            int at = via == "Bluetooth" ? 32 : 30;
            if (r.Length <= at + 1 || r[0] != (via == "Bluetooth" ? 0x11 : 0x01)) return null;
            if (via == "адаптер" && (r[at + 1] & 0x04) != 0) return (NoController, false);
            int data = r[at] & 0x0F;
            bool cable = (r[at] & 0x10) != 0;
            if (!cable) return (data < 10 ? data * 10 + 5 : 100, false);
            return data switch
            {
                < 10 => (data * 10 + 5, true),
                10 or 11 => (100, true),
                _ => (BatteryError, false),
            };
        }
    }
}
