using System;
using System.Collections.Generic;
using System.Linq;
using System.Text;
using System.Text.RegularExpressions;
using BatteryTray.Core;
using BatteryTray.Hid;

namespace BatteryTray.Providers
{
    /// <summary>
    /// Logitech mice, keyboards and other HID++ devices behind a Unifying, Bolt, Lightspeed or Nano receiver
    /// (device slots 1-6) or on a cable (device index 0xFF). Facts from Solaar (github.com/pwr-Solaar/Solaar,
    /// lib/logitech_receiver), the Linux driver hid-logitech-hidpp and LGSTrayBattery:
    /// - Windows splits the HID++ interface into a short (report 0x10, 7 bytes) and a long (report 0x11, 20 bytes)
    ///   collection. Short requests can only be written to the short one, yet HID++ 2.0 replies arrive on the long
    ///   one - so both are read.
    /// - Ping: [10 dev 00 1s 00 00 mark] -> [11 dev 00 1s major minor mark] (HID++ 2.0), or the HID++ 1.0 error
    ///   [10 dev 8F 00 1s code]: 01 = HID++ 1.0 device, 04/09 = offline, 08 = empty slot.
    /// - Root.GetFeature(id): [11 dev 00 0s idHi idLo] -> [index type version]; index 0 = not supported.
    /// - Battery, first feature found: 0x1004 UnifiedBattery GetStatus [soc levelBits chargeStatus extPower];
    ///   0x1000 BatteryStatus [level next status]; 0x1001 BatteryVoltage and 0x1F20 ADC [mV hi, mV lo, flags].
    /// - Name and kind: 0x0005 DeviceName (length, 16-character chunks, type). HID++ 1.0: registers 0x0D/0x07.
    /// Only reads; never writes registers. Not yet verified on hardware by this project.
    /// </summary>
    public sealed class LogitechProvider : IBatteryProvider
    {
        const ushort LogitechVid = 0x046D;
        const byte SoftwareId = 0x0C; // Solaar uses 0B, G HUB 0D, OpenRGB 07: replies are told apart by it
        const int TimeoutMs = 1500;
        const ushort FeatureName = 0x0005;
        static readonly ushort[] BatteryFeatures = { 0x1004, 0x1000, 0x1001, 0x1F20 };

        /// <summary>What was learned about a device in a slot; kept so the slow discovery runs once.</summary>
        sealed class Slot
        {
            public string Name = "";
            public DeviceKind Kind = DeviceKind.Other;
            public bool Hidpp20;
            public byte FeatureIndex;
            public ushort FeatureId;
            public bool HasBattery => !Hidpp20 || FeatureIndex != 0;
        }

        readonly Dictionary<string, Slot> slots = new();

        public string Name => "Logitech";

        public IReadOnlyList<DeviceReading> Poll()
        {
            var result = new Dictionary<string, DeviceReading>();
            var collections = HidBus.Enumerate((v, p) => v == LogitechVid && !(p >= 0x0800 && p <= 0x09FF) // webcams lock up
                    && !HeadsetProvider.HandledIds.Contains(((uint)v << 16) | p))
                // Bluetooth LE devices: Windows reports their battery itself (BluetoothProvider)
                .Where(i => !i.Path.Contains("{00001812-", StringComparison.OrdinalIgnoreCase));

            // One USB interface = one HID++ channel, split by Windows into collections
            foreach (var channel in collections.GroupBy(i => ChannelKey(i.Path)))
            {
                HidInfo? shortC = channel.FirstOrDefault(i => !i.AccessDenied && i.UsagePage >= 0xFF00 && i.OutputLength == 7);
                HidInfo? longC = channel.FirstOrDefault(i => !i.AccessDenied && i.UsagePage >= 0xFF00 && i.OutputLength == 20);
                if (shortC == null && longC == null) continue;
                ushort pid = channel.First().ProductId;
                bool receiver = pid is >= 0xC500 and <= 0xC5FF;
                try
                {
                    using var link = new Link(shortC, longC);
                    var indices = receiver ? new byte[] { 1, 2, 3, 4, 5, 6 } : new byte[] { 0xFF };
                    foreach (byte dev in indices)
                    {
                        DeviceReading? reading = QuerySlot(link, channel.Key, dev, receiver, pid);
                        if (reading == null) continue;
                        if (!result.TryGetValue(reading.Id, out var other) || (reading.Online && !other.Online))
                            result[reading.Id] = reading;
                    }
                }
                catch (Exception e)
                {
                    Log.Write($"logitech {pid:X4}: {e.Message}");
                }
            }
            return result.Values.ToList();
        }

        public TimeSpan Delay(IReadOnlyList<DeviceReading> last)
        {
            return TimeSpan.FromSeconds(60);
        }

        /// <summary>The interface a collection belongs to: Windows numbers collections twice in the path,
        /// ...&amp;mi_02&amp;col01#7&amp;123&amp;0&amp;0000#{guid} and ...&amp;col02#7&amp;123&amp;0&amp;0001#{guid}; both numbers go.</summary>
        static string ChannelKey(string path)
        {
            string noCol = Regex.Replace(path, "&col[0-9a-f]{2}", "", RegexOptions.IgnoreCase);
            return Regex.Replace(noCol, @"&[0-9a-f]{4}(#\{[0-9a-f-]+\})$", "$1", RegexOptions.IgnoreCase).ToLowerInvariant();
        }

        DeviceReading? QuerySlot(Link link, string channel, byte dev, bool receiver, ushort pid)
        {
            string key = channel + "#" + dev;
            PingReply ping = link.Ping(dev);
            if (ping.Result == PingResult.NoDevice && !receiver)
            {
                dev = 0x00; // a directly connected HID++ 1.0 device answers on index 0
                ping = link.Ping(dev);
            }
            if (ping.Result == PingResult.NoDevice)
            {
                slots.Remove(key);
                return null;
            }

            slots.TryGetValue(key, out Slot? slot);
            if (ping.Result == PingResult.Offline)
                return slot != null && slot.HasBattery ? Reading(slot, receiver, null, false, "выключено или спит") : null;

            if (slot == null)
            {
                // Discovery that didn't get every answer (device fell asleep) is retried on the next poll
                slot = Describe(link, dev, ping.Result == PingResult.Hidpp20, pid);
                if (slot == null) return null;
                slots[key] = slot;
            }
            if (!slot.HasBattery) return null;

            var battery = slot.Hidpp20 ? ReadFeature(link, dev, slot) : ReadRegisters(link, dev);
            if (battery == null)
            {
                if (slot.Hidpp20) slots.Remove(key); // look the feature up again next time
                return Reading(slot, receiver, null, false, "заряд не прочитан");
            }
            return battery.Value.Online
                ? Reading(slot, receiver, battery.Value.Level, battery.Value.Charging, null)
                : Reading(slot, receiver, null, false, "выключено или спит");
        }

        static DeviceReading Reading(Slot slot, bool receiver, int? level, bool charging, string? detail)
        {
            return new DeviceReading
            {
                Id = "logitech:" + Regex.Replace(slot.Name.ToLowerInvariant(), "[^a-z0-9]+", "-").Trim('-'),
                Name = slot.Name, Kind = slot.Kind, Via = receiver ? "2.4 ГГц" : "кабель",
                Online = detail == null, Level = level, Charging = charging, Detail = detail,
            };
        }

        static Slot? Describe(Link link, byte dev, bool hidpp20, ushort pid)
        {
            var slot = new Slot { Hidpp20 = hidpp20, Name = $"Logitech {pid:X4}" + (dev is >= 1 and <= 6 ? $" #{dev}" : "") };
            if (!hidpp20) return slot;

            if (link.GetFeature(dev, FeatureName) is not byte nameIndex) return null;
            if (nameIndex != 0)
            {
                if (link.Call(dev, nameIndex, 0) is not byte[] len) return null;
                if (len[0] > 0)
                {
                    var name = new List<byte>();
                    while (name.Count < len[0] && link.Call(dev, nameIndex, 1, (byte)name.Count) is byte[] chunk)
                    {
                        var part = chunk.Take(Math.Min(16, len[0] - name.Count)).ToArray();
                        if (part.Length == 0) break;
                        name.AddRange(part);
                    }
                    string text = Encoding.UTF8.GetString(name.ToArray()).TrimEnd('\0').Trim();
                    if (text.Length > 0) slot.Name = text.StartsWith("Logitech", StringComparison.OrdinalIgnoreCase) ? text : "Logitech " + text;
                }
                if (link.Call(dev, nameIndex, 2) is byte[] type)
                {
                    slot.Kind = type[0] switch
                    {
                        0 or 2 => DeviceKind.Keyboard,
                        3 or 4 or 5 => DeviceKind.Mouse,
                        8 => DeviceKind.Headset,
                        _ => DeviceKind.Other,
                    };
                }
            }

            foreach (ushort feature in BatteryFeatures)
            {
                if (link.GetFeature(dev, feature) is not byte index) return null;
                if (index == 0) continue;
                slot.FeatureIndex = index;
                slot.FeatureId = feature;
                break;
            }
            return slot;
        }

        readonly record struct Battery(bool Online, int? Level, bool Charging);

        static Battery? ReadFeature(Link link, byte dev, Slot slot)
        {
            switch (slot.FeatureId)
            {
                case 0x1004: // UnifiedBattery.GetStatus
                {
                    if (link.Call(dev, slot.FeatureIndex, 1) is not byte[] r) return null;
                    int? level = r[0] != 0 ? r[0] : r[1] switch { 8 => 90, 4 => 50, 2 => 20, 1 => 5, _ => null };
                    return new Battery(true, level, r[2] is 1 or 2 or 3); // 1 charging, 2 slowly, 3 complete (plugged in)
                }
                case 0x1000: // BatteryStatus.GetBatteryLevelStatus
                {
                    if (link.Call(dev, slot.FeatureIndex, 0) is not byte[] r) return null;
                    int? level = r[0] != 0 ? r[0] : r[2] == 3 ? 100 : null;
                    return new Battery(true, level, r[2] is 1 or 2 or 3 or 4);
                }
                case 0x1001: // BatteryVoltage.GetBatteryInfo
                {
                    if (link.Call(dev, slot.FeatureIndex, 0) is not byte[] r) return null;
                    return new Battery(true, VoltageToPercent((r[0] << 8) | r[1]), (r[2] & 0x80) != 0);
                }
                case 0x1F20: // ADC measurement (headsets); bit 0 of the flags = headset active
                {
                    if (link.Call(dev, slot.FeatureIndex, 0) is not byte[] r || (r[2] & 0x01) == 0) return new Battery(false, null, false);
                    return new Battery(true, VoltageToPercent((r[0] << 8) | r[1]), (r[2] & 0x02) != 0);
                }
                default:
                    return null;
            }
        }

        /// <summary>HID++ 1.0 battery registers: 0x0D (percent), then 0x07 (four levels).</summary>
        static Battery? ReadRegisters(Link link, byte dev)
        {
            if (link.ReadRegister(dev, 0x0D) is byte[] charge && charge[0] is > 0 and <= 100)
                return new Battery(true, charge[0], (charge[2] & 0xF0) is 0x50 or 0x90);
            if (link.ReadRegister(dev, 0x07) is byte[] status)
            {
                int? level = status[0] switch { 7 => 90, 5 => 50, 3 => 20, 1 => 5, _ => null };
                bool charging = (status[1] & 0x21) == 0x21 || (status[1] & 0x22) == 0x22;
                return new Battery(true, level, charging);
            }
            return null;
        }

        // Solaar's discharge curve for HID++ voltage readings (mV -> %), interpolated linearly
        static readonly (int Mv, int Percent)[] Curve =
        {
            (4186, 100), (4067, 90), (3989, 80), (3922, 70), (3859, 60), (3811, 50), (3778, 40),
            (3751, 30), (3717, 20), (3671, 10), (3646, 5), (3579, 2), (3500, 0),
        };

        static int VoltageToPercent(int mv)
        {
            if (mv >= Curve[0].Mv) return 100;
            for (int i = 1; i < Curve.Length; i++)
            {
                if (mv < Curve[i].Mv) continue;
                var (hiMv, hiP) = Curve[i - 1];
                var (loMv, loP) = Curve[i];
                return loP + (mv - loMv) * (hiP - loP) / (hiMv - loMv);
            }
            return 0;
        }

        enum PingResult { Hidpp20, Hidpp10, Offline, NoDevice }

        readonly record struct PingReply(PingResult Result);

        /// <summary>One HID++ channel: the short and the long collection of the same interface.</summary>
        sealed class Link : IDisposable
        {
            readonly HidDevice? shortDev, longDev;
            byte mark = 0x5A;

            public Link(HidInfo? shortInfo, HidInfo? longInfo)
            {
                if (shortInfo != null) shortDev = new HidDevice(shortInfo);
                if (longInfo != null) longDev = new HidDevice(longInfo);
            }

            static bool SameDevice(byte[] r, byte dev) => r[1] == dev || r[1] == (dev ^ 0xFF);

            public PingReply Ping(byte dev)
            {
                mark++;
                byte fn = 0x10 | SoftwareId;
                byte[]? r = Request(new byte[] { 0x10, dev, 0x00, fn, 0, 0, mark }, x => x.Length >= 7 && SameDevice(x, dev)
                    && ((x[2] == 0x00 && x[3] == fn && x[6] == mark) || (x[2] == 0x8F && x[3] == 0x00 && x[4] == fn)));
                if (r == null) return new PingReply(PingResult.Offline);
                if (r[2] == 0x00) return new PingReply(PingResult.Hidpp20);
                return r[5] switch
                {
                    0x01 => new PingReply(PingResult.Hidpp10),
                    0x08 => new PingReply(PingResult.NoDevice),
                    _ => new PingReply(PingResult.Offline),
                };
            }

            /// <summary>The feature's index, 0 when the device doesn't have it, null when it didn't answer.</summary>
            public byte? GetFeature(byte dev, ushort featureId)
            {
                return Call(dev, 0x00, 0, (byte)(featureId >> 8), (byte)featureId) is byte[] r ? r[0] : null;
            }

            /// <summary>A HID++ 2.0 function call; returns the 16 reply parameter bytes, or null on error/timeout.</summary>
            public byte[]? Call(byte dev, byte featureIndex, int function, params byte[] parameters)
            {
                byte fn = (byte)((function << 4) | SoftwareId);
                var request = new byte[20];
                request[0] = 0x11;
                request[1] = dev;
                request[2] = featureIndex;
                request[3] = fn;
                Array.Copy(parameters, 0, request, 4, Math.Min(parameters.Length, 16));
                for (int attempt = 0; attempt < 3; attempt++)
                {
                    byte[]? r = Request(request, x => x.Length >= 7 && SameDevice(x, dev)
                        && ((x[2] == featureIndex && x[3] == fn) || ((x[2] == 0xFF || x[2] == 0x8F) && x[3] == featureIndex && x[4] == fn)));
                    if (r == null) return null;
                    if (r[2] == featureIndex) return r.Length >= 20 ? r[4..20] : Pad16(r[4..]);
                    if (!(r[2] == 0xFF && r[5] == 0x08)) return null; // an error other than "busy"
                }
                return null;
            }

            /// <summary>HID++ 1.0 short register read; returns the three value bytes.</summary>
            public byte[]? ReadRegister(byte dev, byte register)
            {
                byte[]? r = Request(new byte[] { 0x10, dev, 0x81, register, 0, 0, 0 }, x => x.Length >= 7 && SameDevice(x, dev)
                    && ((x[2] == 0x81 && x[3] == register) || (x[2] == 0x8F && x[3] == 0x81 && x[4] == register)));
                return r != null && r[2] == 0x81 ? r[4..7] : null;
            }

            static byte[] Pad16(byte[] b)
            {
                var r = new byte[16];
                Array.Copy(b, r, Math.Min(b.Length, 16));
                return r;
            }

            byte[]? Request(byte[] request, Func<byte[], bool> isReply)
            {
                HidDevice? target = request[0] == 0x10 ? shortDev : longDev;
                if (target == null && request[0] == 0x10 && longDev != null)
                {
                    // No short collection (some Bluetooth/direct devices): send the same request long
                    var asLong = new byte[20];
                    Array.Copy(request, asLong, request.Length);
                    asLong[0] = 0x11;
                    request = asLong;
                    target = longDev;
                }
                if (target == null) return null;

                shortDev?.Flush();
                longDev?.Flush();
                if (!target.Write(request, 500)) return null;

                // Replies can come on either collection; each keeps its own queue, so polling them in turn loses nothing
                DateTime deadline = DateTime.UtcNow.AddMilliseconds(TimeoutMs);
                while (DateTime.UtcNow < deadline)
                {
                    foreach (HidDevice? d in new[] { longDev, shortDev })
                    {
                        if (d == null) continue;
                        byte[]? r = d.Read(15);
                        if (r != null && isReply(r)) return r;
                    }
                }
                return null;
            }

            public void Dispose()
            {
                shortDev?.Dispose();
                longDev?.Dispose();
            }
        }
    }
}
