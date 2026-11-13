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
    ///   Aerox 3 Wireless Gen 2: the same commands, a different reply: [command, status, level, 0], status 0 on
    ///     battery, 1 charging, 2 charged, the level in percent. The level is FF for about a minute after the mouse
    ///     goes on or off the cable, and an asleep or switched-off mouse answers 40 FF 00 00 instead (rivalcfg issue
    ///     276 and its branch device_aerox3_wireless_gen2, from a Gen 2 owner's logs on Linux).
    ///   Rival 3 Wireless (and Gen 2), Rival 650 Wireless: AA 01; reply [level, ?, charging].
    /// A Gen 1 Aerox or Rival with a level outside 0-100 is off. Read-only; not yet verified on hardware by this project.
    /// </summary>
    public sealed class SteelSeriesProvider : IBatteryProvider
    {
        const ushort SteelSeriesVid = 0x1038;
        const int TimeoutMs = 300;

        internal enum Protocol { Aerox, AeroxGen2, Rival }

        /// <summary>What a mouse answered. <see cref="Level"/> is null while the mouse doesn't know it yet.</summary>
        internal sealed record Battery(int? Level, bool Charging, bool Full);

        sealed record Model(string Key, string Name, int Interface, Protocol Protocol, bool Wireless);

        static readonly Dictionary<ushort, Model> Models = new()
        {
            [0x1838] = new("aerox3", "SteelSeries Aerox 3 Wireless", 3, Protocol.Aerox, true),
            [0x1878] = new("aerox3", "SteelSeries Aerox 3 Wireless", 3, Protocol.Aerox, true),
            [0x183A] = new("aerox3", "SteelSeries Aerox 3 Wireless", 3, Protocol.Aerox, false),
            [0x187A] = new("aerox3", "SteelSeries Aerox 3 Wireless", 3, Protocol.Aerox, false),
            [0x1890] = new("aerox3gen2", "SteelSeries Aerox 3 Wireless Gen 2", 3, Protocol.AeroxGen2, true),
            [0x1892] = new("aerox3gen2", "SteelSeries Aerox 3 Wireless Gen 2", 3, Protocol.AeroxGen2, false),
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

        readonly HashSet<string> logged = new();

        public string Name => "SteelSeries";

        public IReadOnlyList<DeviceReading> Poll()
        {
            var result = new List<DeviceReading>();
            var found = HidBus.Enumerate((v, p) => v == SteelSeriesVid && Models.ContainsKey(p));
            foreach (var group in found.GroupBy(i => i.ProductId))
            {
                Model model = Models[group.Key];
                List<HidInfo> collections = group.ToList();
                // The vendor collection of the configuration interface; a single-interface device has no "&mi_"
                var candidates = collections.Where(i => Math.Max(i.InterfaceNumber, 0) == model.Interface).ToList();
                var asked = new List<(HidInfo Info, List<string> Trace)>();
                Battery? battery = null;
                string? error = null;
                foreach (HidInfo info in candidates)
                {
                    if (info.AccessDenied)
                    {
                        error = Loc.T("d.needsAdmin");
                        continue;
                    }
                    if (info.UsagePage < 0xFF00 || info.OutputLength == 0 || info.InputLength == 0) continue;
                    var trace = new List<string>();
                    asked.Add((info, trace));
                    try
                    {
                        battery = Query(info, model, trace);
                    }
                    catch (Exception e)
                    {
                        trace.Add("error: " + e.Message);
                        error = Loc.T("d.error", e.Message);
                    }
                    if (battery != null) break;
                }
                LogOnce(group.Key, collections, asked, battery != null);

                result.Add(new DeviceReading
                {
                    Id = "steelseries:" + model.Key, Name = model.Name, Kind = DeviceKind.Mouse,
                    Via = Loc.T(model.Wireless ? "via.24" : "via.cable"), Online = battery != null,
                    Level = battery?.Level, Charging = battery?.Charging ?? false, FullOnCharger = battery?.Full ?? false,
                    Detail = battery == null ? error ?? Loc.T("d.asleep") : battery.Level == null ? Loc.T("d.levelUnknown") : null,
                });
            }
            // A mouse charging on its cable while its receiver stays plugged in is listed twice under one Id, and the
            // receiver finds it off. The window keeps one reading per Id, so it gets the one that reached the mouse.
            return result.GroupBy(r => r.Id).Select(g => g.FirstOrDefault(r => r.Online) ?? g.First()).ToList();
        }

        public TimeSpan Delay(IReadOnlyList<DeviceReading> last)
        {
            // A Gen 2 that doesn't know its level yet will within a minute of going on or off the cable
            if (last.Any(r => r.Online && r.Level == null)) return TimeSpan.FromSeconds(20);
            return last.Any(r => r.Online) ? TimeSpan.FromSeconds(60) : TimeSpan.FromSeconds(20);
        }

        static Battery? Query(HidInfo info, Model model, List<string> trace)
        {
            using var dev = new HidDevice(info);
            dev.Flush();
            byte command = model.Protocol == Protocol.Rival ? (byte)0xAA : model.Wireless ? (byte)0xD2 : (byte)0x92;
            // Byte 0 is the report ID (the device uses none)
            byte[] report = model.Protocol == Protocol.Rival ? new byte[] { 0, 0xAA, 0x01 } : new byte[] { 0, command };
            trace.Add("> " + Hex(report));
            if (!dev.Write(report, TimeoutMs))
            {
                trace.Add("(not sent)");
                return null;
            }

            // rivalcfg takes the next input report. If the reply echoes the command, prefer that one, so an
            // unrelated event report arriving first can't be mistaken for the answer.
            byte[]? first = null;
            DateTime deadline = DateTime.UtcNow.AddMilliseconds(TimeoutMs);
            while (DateTime.UtcNow < deadline)
            {
                byte[]? r = dev.Read(Math.Max((int)(deadline - DateTime.UtcNow).TotalMilliseconds, 1));
                if (r == null)
                {
                    trace.Add("< nothing");
                    break;
                }
                trace.Add("< " + Hex(r));
                first ??= r;
                if (r.Length > 1 && r[1] == command) return Parse(r, model.Protocol, command);
            }
            return first != null ? Parse(first, model.Protocol, command) : null;
        }

        /// <summary>Reads the battery from an input report as Windows returns it, report ID first; null = the mouse is off.</summary>
        internal static Battery? Parse(byte[] r, Protocol protocol, byte command)
        {
            // r[0] is the report ID; rivalcfg's data[n] is r[n + 1]
            switch (protocol)
            {
                case Protocol.Aerox:
                {
                    if (r.Length < 3) return null;
                    int level = ((r[2] & 0x7F) - 1) * 5;
                    return level is >= 0 and <= 100 ? new Battery(level, (r[2] & 0x80) != 0, false) : null;
                }
                case Protocol.AeroxGen2:
                {
                    // Anything but the echo of the command, 40 FF 00 00 included, means the mouse is asleep or off
                    if (r.Length < 4 || r[1] != command) return null;
                    byte status = r[2], level = r[3];
                    return new Battery(level <= 100 ? level : null, status == 1, status == 2);
                }
                default:
                {
                    if (r.Length < 4) return null;
                    int level = r[1];
                    return level is >= 0 and <= 100 ? new Battery(level, r[3] != 0, false) : null;
                }
            }
        }

        /// <summary>
        /// Logs how a mouse was queried - its HID collections, the request and the replies - the first time it is
        /// found off and the first time it is found on in a session, as the headsets do. None of these mice is tested
        /// yet, so when one isn't reached or reads wrong, these lines show which collection answered and with what bytes.
        /// </summary>
        void LogOnce(ushort productId, List<HidInfo> collections, List<(HidInfo Info, List<string> Trace)> asked, bool online)
        {
            string id = $"{SteelSeriesVid:x4}:{productId:x4}";
            if (!logged.Add(id + online)) return;
            Log.Write($"SteelSeries {id}: found {(online ? "on" : "off")}, {collections.Count} HID collections");
            foreach (HidInfo info in collections)
            {
                List<string>? trace = asked.FirstOrDefault(a => a.Info == info).Trace;
                Log.Write($"  mi={info.InterfaceNumber} {info}: {(trace == null ? "not asked" : string.Join("  ", trace))}");
            }
        }

        /// <summary>
        /// A report for the log as rivalcfg prints it: without the report ID in front and the zeros that pad it, and
        /// no more than its first 32 bytes.
        /// </summary>
        static string Hex(byte[] report)
        {
            int used = Array.FindLastIndex(report, b => b != 0) + 1;
            if (used <= 1) return "00";
            int shown = Math.Min(used, 33) - 1;
            string hex = BitConverter.ToString(report, 1, shown).Replace('-', ' ').ToLowerInvariant();
            return used > 33 ? hex + " ..." : hex;
        }
    }
}
