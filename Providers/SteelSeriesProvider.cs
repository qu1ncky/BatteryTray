using System;
using System.Collections.Generic;
using System.Linq;
using BatteryTray.Core;
using BatteryTray.Hid;

namespace BatteryTray.Providers
{
    /// <summary>
    /// SteelSeries wireless mice, over their 2.4 GHz receiver or the cable. The battery command is an output
    /// report on the mouse's vendor interface, answered by an input report. Commands from rivalcfg
    /// (github.com/flozz/rivalcfg):
    ///   Aerox 3/5/9 Wireless, Prime (Mini) Wireless: 0x92 over the cable, 0xD2 (0x92 | 0x40) over 2.4 GHz;
    ///     the reply's second data byte b holds charging in bit 7 and level = ((b &amp; 0x7F) - 1) * 5.
    ///   Rival 3 Wireless (and Gen 2), Rival 650 Wireless: AA 01; reply [level, ?, charging].
    /// A level outside 0-100 means the mouse is off. Read-only; not yet verified on hardware by this project.
    /// </summary>
    public sealed class SteelSeriesProvider : IBatteryProvider
    {
        const ushort SteelSeriesVid = 0x1038;
        const int TimeoutMs = 300;

        enum Protocol { Aerox, Rival }

        sealed record Model(string Key, string Name, int Interface, Protocol Protocol, bool Wireless);

        static readonly Dictionary<ushort, Model> Models = new()
        {
            [0x1838] = new("aerox3", "SteelSeries Aerox 3 Wireless", 3, Protocol.Aerox, true),
            [0x1878] = new("aerox3", "SteelSeries Aerox 3 Wireless", 3, Protocol.Aerox, true),
            [0x183A] = new("aerox3", "SteelSeries Aerox 3 Wireless", 3, Protocol.Aerox, false),
            [0x187A] = new("aerox3", "SteelSeries Aerox 3 Wireless", 3, Protocol.Aerox, false),
            [0x1852] = new("aerox5", "SteelSeries Aerox 5 Wireless", 3, Protocol.Aerox, true),
            [0x185C] = new("aerox5", "SteelSeries Aerox 5 Wireless", 3, Protocol.Aerox, true),
            [0x1860] = new("aerox5", "SteelSeries Aerox 5 Wireless", 3, Protocol.Aerox, true),
            [0x1854] = new("aerox5", "SteelSeries Aerox 5 Wireless", 3, Protocol.Aerox, false),
            [0x185E] = new("aerox5", "SteelSeries Aerox 5 Wireless", 3, Protocol.Aerox, false),
            [0x1862] = new("aerox5", "SteelSeries Aerox 5 Wireless", 3, Protocol.Aerox, false),
            [0x1858] = new("aerox9", "SteelSeries Aerox 9 Wireless", 3, Protocol.Aerox, true),
            [0x1874] = new("aerox9", "SteelSeries Aerox 9 Wireless", 3, Protocol.Aerox, true),
            [0x185A] = new("aerox9", "SteelSeries Aerox 9 Wireless", 3, Protocol.Aerox, false),
            [0x1876] = new("aerox9", "SteelSeries Aerox 9 Wireless", 3, Protocol.Aerox, false),
            [0x1840] = new("prime", "SteelSeries Prime Wireless", 3, Protocol.Aerox, true),
            [0x1842] = new("prime", "SteelSeries Prime Wireless", 3, Protocol.Aerox, false),
            [0x1848] = new("primemini", "SteelSeries Prime Mini Wireless", 3, Protocol.Aerox, true),
            [0x184A] = new("primemini", "SteelSeries Prime Mini Wireless", 3, Protocol.Aerox, false),
            [0x1830] = new("rival3", "SteelSeries Rival 3 Wireless", 3, Protocol.Rival, true),
            [0x1872] = new("rival3gen2", "SteelSeries Rival 3 Wireless Gen 2", 3, Protocol.Rival, true),
            [0x1726] = new("rival650", "SteelSeries Rival 650 Wireless", 0, Protocol.Rival, true),
            [0x172B] = new("rival650", "SteelSeries Rival 650 Wireless", 0, Protocol.Rival, false),
        };

        public string Name => "SteelSeries";

        public IReadOnlyList<DeviceReading> Poll()
        {
            var result = new List<DeviceReading>();
            var found = HidBus.Enumerate((v, p) => v == SteelSeriesVid && Models.ContainsKey(p));
            foreach (var group in found.GroupBy(i => i.ProductId))
            {
                Model model = Models[group.Key];
                // The vendor collection of the configuration interface; a single-interface device has no "&mi_"
                var candidates = group.Where(i => Math.Max(i.InterfaceNumber, 0) == model.Interface).ToList();
                (int Level, bool Charging)? battery = null;
                string? error = null;
                foreach (HidInfo info in candidates)
                {
                    if (info.AccessDenied)
                    {
                        error = Loc.T("d.needsAdmin");
                        continue;
                    }
                    if (info.UsagePage < 0xFF00 || info.OutputLength == 0 || info.InputLength == 0) continue;
                    try
                    {
                        battery = Query(info, model);
                    }
                    catch (Exception e)
                    {
                        error = Loc.T("d.error", e.Message);
                    }
                    if (battery != null) break;
                }

                result.Add(new DeviceReading
                {
                    Id = "steelseries:" + model.Key, Name = model.Name, Kind = DeviceKind.Mouse,
                    Via = Loc.T(model.Wireless ? "via.24" : "via.cable"), Online = battery != null,
                    Level = battery?.Level, Charging = battery?.Charging ?? false,
                    Detail = battery != null ? null : error ?? Loc.T("d.asleep"),
                });
            }
            return result;
        }

        public TimeSpan Delay(IReadOnlyList<DeviceReading> last)
        {
            return last.Any(r => r.Online) ? TimeSpan.FromSeconds(60) : TimeSpan.FromSeconds(20);
        }

        static (int Level, bool Charging)? Query(HidInfo info, Model model)
        {
            using var dev = new HidDevice(info);
            dev.Flush();
            byte command = model.Protocol == Protocol.Rival ? (byte)0xAA : model.Wireless ? (byte)0xD2 : (byte)0x92;
            // Byte 0 is the report ID (the device uses none)
            byte[] report = model.Protocol == Protocol.Rival ? new byte[] { 0, 0xAA, 0x01 } : new byte[] { 0, command };
            if (!dev.Write(report, TimeoutMs)) return null;

            // rivalcfg takes the next input report. If the reply echoes the command, prefer that one, so an
            // unrelated event report arriving first can't be mistaken for the answer.
            byte[]? first = null;
            DateTime deadline = DateTime.UtcNow.AddMilliseconds(TimeoutMs);
            while (DateTime.UtcNow < deadline)
            {
                byte[]? r = dev.Read(Math.Max((int)(deadline - DateTime.UtcNow).TotalMilliseconds, 1));
                if (r == null) break;
                first ??= r;
                if (r.Length > 1 && r[1] == command) return Parse(r, model);
            }
            return first != null ? Parse(first, model) : null;
        }

        static (int Level, bool Charging)? Parse(byte[] r, Model model)
        {
            // r[0] is the report ID; rivalcfg's data[n] is r[n + 1]
            int level;
            bool charging;
            if (model.Protocol == Protocol.Aerox)
            {
                if (r.Length < 3) return null;
                level = ((r[2] & 0x7F) - 1) * 5;
                charging = (r[2] & 0x80) != 0;
            }
            else
            {
                if (r.Length < 4) return null;
                level = r[1];
                charging = r[3] != 0;
            }
            return level is >= 0 and <= 100 ? (level, charging) : null;
        }
    }
}
