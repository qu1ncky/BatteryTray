using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using BatteryTray.Core;
using BatteryTray.Hid;

namespace BatteryTray.Providers
{
    /// <summary>
    /// Wireless gaming headsets that answer a battery request on a vendor HID collection: HyperX, Logitech G,
    /// SteelSeries Arctis, Corsair, Sony INZONE and a few more. The request/response bytes are the protocol facts
    /// collected by HeadsetControl (github.com/Sapd/HeadsetControl, lib/devices/*); this is an independent
    /// implementation. Byte positions below are as hidapi returns them: for devices without report IDs Windows puts
    /// a 0x00 in front of every input report, which <see cref="Io.Data"/> strips the same way hidapi does.
    /// Read-only status queries; not yet verified on hardware by this project.
    /// </summary>
    public sealed class HeadsetProvider : IBatteryProvider
    {
        const int TimeoutMs = 2000;

        /// <param name="Detail">A text key (see <see cref="Loc"/>) or a ready text.</param>
        readonly record struct State(bool Online, int? Level, bool Charging, string? Detail = null);

        static readonly State Off = new(false, null, false, "d.offOrAway");

        static State On(int level, bool charging) => new(true, Math.Clamp(level, 0, 100), charging);

        /// <param name="Page">Usage page of the collection to talk to; 0 = any vendor collection.</param>
        /// <param name="Interface">USB interface number; -1 = any.</param>
        sealed record Spec(string Key, string Name, ushort Vid, ushort[] Pids, ushort Page, ushort Usage, int Interface,
            Func<Io, State> Query);

        // Voltage (mV) -> percent tables HeadsetControl uses for the Logitech headsets
        static readonly (int Mv, int Percent)[] G533 = { (4200, 100), (3850, 50), (3790, 30), (3750, 20), (3680, 5), (3330, 0) };
        static readonly (int Mv, int Percent)[] G535 = { (4175, 100), (3817, 50), (3766, 30), (3730, 20), (3664, 5), (3310, 0) };
        static readonly (int Mv, int Percent)[] G633 =
            { (4100, 100), (3950, 80), (3850, 60), (3750, 40), (3650, 20), (3500, 10), (3300, 5), (3150, 0) };
        static readonly (int Mv, int Percent)[] GPro = { (4150, 100), (3830, 50), (3780, 30), (3740, 20), (3670, 5), (3320, 0) };

        static readonly Spec[] Specs =
        {
            // ---- HyperX ----
            new("hyperx-cloud-alpha-wireless", "HyperX Cloud Alpha Wireless", 0x03F0, new ushort[] { 0x098D }, 0, 0, -1, io =>
            {
                // Mute events 21 BB 23 arrive on the same collection: match the command echo
                byte[]? Ask(byte cmd) => io.Ask(new byte[] { 0x21, 0xBB, cmd }, d => d.Length > 3 && d[0] == 0x21 && d[1] == 0xBB && d[2] == cmd);
                byte[]? link = Ask(0x03);
                if (link == null || link[3] == 0x01) return Off;
                bool charging = Ask(0x0C) is byte[] c && c[3] == 0x01;
                return Ask(0x0B) is byte[] b ? On(b[3], charging) : Off;
            }),
            new("hyperx-cloud-ii-wireless", "HyperX Cloud II Wireless", 0x03F0, new ushort[] { 0x0696, 0x018B }, 0xFF90, 0x0303, -1, io =>
            {
                byte[]? Ask(byte cmd) => io.Ask(Pad(52, 0x06, 0xFF, 0xBB, cmd),
                    d => d.Length >= 8 && d[0] == 0x06 && d[1] == 0xFF && d[2] == 0xBB && d[3] == cmd);
                if (Ask(0x02) is not byte[] b) return Off;
                return On(b[7], Ask(0x03) is byte[] c && c[4] == 0x01);
            }),
            new("hyperx-cloud-ii-wireless", "HyperX Cloud II Wireless", 0x0951, new ushort[] { 0x1718 }, 0, 0, -1, io =>
            {
                byte[]? Ask(byte cmd)
                {
                    io.GetInputReport(0x06); // HeadsetControl wakes the receiver this way; the reply is ignored
                    return io.Ask(Pad(62, 0x06, 0x00, 0x02, 0x00, 0x9A, 0x00, 0x00, 0x68, 0x4A, 0x8E, 0x0A, 0, 0, 0, 0xBB, cmd),
                        d => d.Length >= 8 && d[0] == 0x0B && d[2] == 0xBB && d[3] == cmd);
                }
                if (Ask(0x02) is not byte[] b) return Off;
                return On(b[7], Ask(0x03) is byte[] c && c[4] == 0x01);
            }),
            new("hyperx-cloud-flight", "HyperX Cloud Flight", 0x0951, new ushort[] { 0x16C4, 0x1723 }, 0, 0, -1, io =>
            {
                // 64 xx (power) and 65 xx (mute) are events, not the reply
                if (io.Ask(Pad(20, 0x21, 0xFF, 0x05), d => d.Length >= 5 && d[0] is not (0x64 or 0x65)) is not byte[] r) return Off;
                int mv = (r[3] << 8) | r[4];
                if (mv > 4107) return new State(true, null, true); // charging: the voltage says nothing then
                if (mv < 3000) return Off;
                double v = mv;
                double percent = mv <= 3648 ? 0.00125 * v
                    : mv > 3975 ? 100
                    : 2.547505e-8 * Math.Pow(v, 4) - 3.900299e-4 * Math.Pow(v, 3) + 2.238321 * v * v - 5706.256 * v + 5452299;
                return On((int)Math.Round(percent), false);
            }),

            // ---- Logitech G (HID++ with a fixed feature index; the reply carries the battery voltage) ----
            new("logitech-g533", "Logitech G533", 0x046D, new ushort[] { 0x0A66 }, 0xFF43, 0x0202, 3,
                io => LogitechVoltage(io, 0x07, 0x01, G533)),
            new("logitech-g535", "Logitech G535", 0x046D, new ushort[] { 0x0AC4 }, 0x000C, 0x0001, 3,
                io => LogitechVoltage(io, 0x05, 0x0D, G535)),
            new("logitech-g-headset", "Logitech G633 / G635 / G733 / G933 / G935", 0x046D,
                new ushort[] { 0x0A5C, 0x0A89, 0x0A5B, 0x0A87, 0x0AB5, 0x0AFE, 0x0B1F }, 0xFF43, 0x0202, -1,
                io => LogitechVoltage(io, 0x08, 0x0A, G633)),
            new("logitech-g-pro", "Logitech G PRO / PRO X Wireless", 0x046D,
                new ushort[] { 0x0AA7, 0x0AAA, 0x0ABA, 0x0AFB, 0x0AFC }, 0xFF43, 0x0202, -1,
                io => LogitechVoltage(io, 0x06, 0x0D, GPro)),
            new("logitech-g930", "Logitech G930", 0x046D, new ushort[] { 0x0A1F }, 0xFF00, 0x0001, -1, io =>
            {
                if (!io.SetFeature(Pad(64, 0xFF, 0x09, 0x00, 0xFD, 0xF4, 0x10, 0x05, 0xB1, 0xBF, 0xA0, 0x04))) return Off;
                byte[]? r = null;
                for (int i = 0; i < 3; i++)
                {
                    Thread.Sleep(100);
                    r = io.GetFeature(0xFF) ?? r;
                }
                return r != null && r.Length > 13 ? On(Map(r[13], 44, 91, 0, 100), false) : Off;
            }),
            new("astro-a50", "ASTRO A50", 0x046D, new ushort[] { 0x0B1C }, 0xFF32, 0x0074, -1, io =>
            {
                var r = io.Ask(Pad(64, 0x02, 0x0C, 0x03, 0x00, 0x06, 0x0C),
                    d => d.Length > 8 && d[0] == 0x02 && d[1] == 0x0C && d[4] == 0x06, reads: 8);
                return r != null ? On(r[6], r[8] != 0) : Off;
            }),

            // ---- SteelSeries Arctis ----
            new("arctis-1", "SteelSeries Arctis 1 Wireless", 0x1038, new ushort[] { 0x12B3, 0x12B6, 0x12D7, 0x12D5 }, 0xFF43, 0x0202, 3,
                io => io.Ask(new byte[] { 0x06, 0x12 }, d => d.Length > 3) is byte[] r && r[2] != 0x01 ? On(r[3], false) : Off),
            new("arctis-7", "SteelSeries Arctis 7 / Arctis Pro (2019)", 0x1038, new ushort[] { 0x1260, 0x12AD, 0x1252, 0x1280 }, 0, 0, 5,
                io => io.Ask(new byte[] { 0x06, 0x18 }, d => d.Length > 2) is byte[] r ? On(r[2], false) : Off),
            new("arctis-9", "SteelSeries Arctis 9", 0x1038, new ushort[] { 0x12C2 }, 0, 0, -1, io =>
            {
                if (io.Ask(new byte[] { 0x00, 0x20 }, d => d.Length > 4) is not byte[] r || r[4] == 0xFF) return Off;
                return On(Map(r[3], 0x64, 0x9A, 0, 100), r[4] == 0x01);
            }),
            new("arctis-pro-wireless", "SteelSeries Arctis Pro Wireless", 0x1038, new ushort[] { 0x1290 }, 0, 0, -1, io =>
            {
                if (io.Ask(Pad(31, 0x41, 0xAA), d => d.Length > 0) is not byte[] link || link[0] == 0x02) return Off;
                return io.Ask(Pad(31, 0x40, 0xAA), d => d.Length > 0) is byte[] r ? On(Map(r[0], 0, 4, 0, 100), false) : Off;
            }),
            new("arctis-nova-pro-wireless", "SteelSeries Arctis Nova Pro Wireless", 0x1038, new ushort[] { 0x12E0, 0x12E5 }, 0, 0, 4, io =>
            {
                // The base station also streams volume / chat-mix / EQ frames: take the B0 status reply only
                if (io.Ask(Pad(31, 0x06, 0xB0), d => d.Length >= 16 && d[1] == 0xB0) is not byte[] r || r[15] == 0x01) return Off;
                return On(Map(r[6], 0, 8, 0, 100), r[15] == 0x02);
            }),
            new("arctis-7-plus", "SteelSeries Arctis 7+", 0x1038, new ushort[] { 0x220E, 0x2212, 0x2216, 0x2236 }, 0xFFC0, 0x0001, 3, io =>
            {
                if (NovaStatus(io) is not byte[] r || r[1] == 0x01) return Off;
                return On(Map(r[2], 0, 4, 0, 100), r[3] == 0x01);
            }),
            new("arctis-nova-7", "SteelSeries Arctis Nova 7", 0x1038,
                new ushort[] { 0x2202, 0x22A1, 0x227E, 0x2206, 0x2258, 0x229E, 0x22AD, 0x223A, 0x22A9, 0x227A, 0x22A4, 0x22A5, 0x220A, 0x22A7, 0x2298 },
                0xFFC0, 0x0001, 3, io =>
                {
                    if (NovaStatus(io) is not byte[] r || r[3] == 0x00) return Off;
                    // Older firmware reports five steps (0-4), newer ones a percentage
                    bool steps = io.ProductId is 0x2202 or 0x2206 or 0x220A or 0x223A or 0x227A or 0x22A4;
                    return On(steps ? Map(r[2], 0, 4, 0, 100) : r[2], r[3] is 0x01 or 0x02);
                }),
            new("arctis-nova-5", "SteelSeries Arctis Nova 5 / Nova 3P", 0x1038, new ushort[] { 0x2232, 0x2253, 0x2269, 0x226D }, 0xFFC0, 0x0001, 3, io =>
            {
                if (NovaStatus(io) is not byte[] r || r[1] == 0x02) return Off;
                return On(r[3], r[4] == 0x01);
            }),
            new("arctis-gamebuds", "SteelSeries Arctis GameBuds", 0x1038, new ushort[] { 0x230A }, 0xFFC0, 0x0001, 3, io =>
            {
                if (NovaStatus(io) is not byte[] r || r.Length < 7) return Off;
                // 03 = out of the case; a bud in the case (02) reports 0% and no charging state
                var buds = new[] { (Status: r[3], Level: r[5]), (Status: r[4], Level: r[6]) }.Where(b => b.Status == 0x03).ToList();
                if (buds.Count == 0) return Off;
                return On(buds.Min(b => b.Level), false);
            }),

            // ---- Corsair ----
            new("corsair-void", "Corsair Void / HS70", 0x1B1C,
                new ushort[] { 0x1B1C, 0x1B27, 0x0A14, 0x0A16, 0x0A17, 0x0A1D, 0x0A1A, 0x1B2A, 0x1B23, 0x1B29, 0x0A55, 0x0A51, 0x0A52, 0x0A38, 0x0A4F, 0x0A2B, 0x0A75, 0x0A56 },
                0xFFC5, 0x0001, 3, io =>
                {
                    if (io.Ask(new byte[] { 0xC9, 0x64 }, d => d.Length > 4 && d[0] == 0x64) is not byte[] r || r[4] == 0) return Off;
                    return On(r[2] & 0x7F, r[4] is 4 or 5);
                }),
            new("corsair-wireless-v2", "Corsair Void / Virtuoso / HS80 MAX Wireless", 0x1B1C, new ushort[] { 0x2A08, 0x2A02, 0x0A97 }, 0, 0, 4, io =>
            {
                // Read-only "get" requests: 00 02 <endpoint: 08 receiver, 09 headset> 02 <property>
                byte[] Get(byte endpoint, byte property) => Pad(65, 0x00, 0x02, endpoint, 0x02, property);
                io.Send(Get(0x08, 0x13));
                io.Send(Get(0x08, 0x12));
                io.Drain();
                if (io.Ask(Get(0x09, 0x12), d => d.Length > 0) == null) return Off;
                io.Drain();
                for (int attempt = 0; attempt < 3; attempt++)
                {
                    if (io.Ask(Get(0x09, 0x0F), d => d.Length > 5) is not byte[] r) return Off;
                    int value = r[4] | (r[5] << 8);
                    if (value is >= 1 and <= 1000) return On(value / 10, false);
                }
                return Off;
            }),
            new("corsair-virtuoso-xt", "Corsair Virtuoso XT / SE", 0x1B1C, new ushort[] { 0x0A64, 0x0A62, 0x0A3E, 0x0A3D }, 0xFF42, 0x0001, 3, io =>
            {
                var r = io.Ask(new byte[] { 0x02, 0x00 }, d => d.Length > 2 && d[0] == 0x01, reads: 8);
                return r != null && r[1] != 0x00 && r[2] <= 100 ? On(r[2], false) : Off;
            }),

            // ---- Others ----
            new("sony-inzone-h5", "Sony INZONE H5", 0x054C, new ushort[] { 0x0EBF }, 0xFF04, 0x0002, -1, io =>
            {
                var request = Pad(64, 0x02, 0x0C, 0x01, 0x00, 0xFC, 0x08, 0x96, 0xC3, 0x41, 0x04, 0x01, 0x02, 0x00);
                byte sum = 0;
                for (int i = 6; i <= 12; i++) sum += request[i];
                request[13] = sum;
                // Our transaction ID (02 00) and a return / active-notification type, so unsolicited events don't count
                var r = io.Ask(request, d => d.Length > 14 && d[0] == 0x02 && d[1] >= 14 && d[2] == 0x04 && d[3] == 0xFF
                    && d[6] == 0x96 && d[7] == 0xC3 && (d[8] >> 4) == 1 && d[9] == 0x04 && d[10] is 0x10 or 0xA0
                    && d[11] == 0x02 && d[12] == 0x00, reads: 10);
                return r != null && r[14] != 0xFF ? On(r[14], r[13] != 0) : Off;
            }),
            new("mchose-x9", "MCHOSE X9", 0x3837, new ushort[] { 0x6045 }, 0xFF90, 0x0001, -1, io =>
            {
                var r = io.Ask(Pad(64, 0x55, 0x65, 0x01), d => d.Length > 3 && d[0] == 0x55 && d[1] == 0x65 && d[3] == 0x02, reads: 32);
                return r != null ? On(r[2], false) : Off;
            }),
            new("lenovo-wireless-voip", "Lenovo Wireless VoIP Headset", 0x17EF, new ushort[] { 0xA07D }, 0xFF07, 0x0222, 3, io =>
            {
                if (!io.SetFeature(Pad(61, 0x24, 0x01))) return Off;
                var r = io.Receive(d => d.Length > 7 && d[0] == 0x27 && d[1] == 0x01);
                return r != null && r[2] == 0 ? On(r[7], false) : Off;
            }),
        };

        /// <summary>Product IDs handled here, so the general Logitech HID++ provider leaves them alone.</summary>
        public static readonly HashSet<uint> HandledIds =
            Specs.SelectMany(s => s.Pids.Select(p => ((uint)s.Vid << 16) | p)).ToHashSet();

        public string Name => "Headsets";

        public IReadOnlyList<DeviceReading> Poll()
        {
            var result = new Dictionary<string, DeviceReading>();
            var found = HidBus.Enumerate((v, p) => HandledIds.Contains(((uint)v << 16) | p));
            foreach (var device in found.GroupBy(i => (i.VendorId, i.ProductId)))
            {
                Spec spec = Specs.First(s => s.Vid == device.Key.VendorId && s.Pids.Contains(device.Key.ProductId));
                State state = Query(spec, device.ToList());
                var reading = new DeviceReading
                {
                    Id = "headset:" + spec.Key, Name = spec.Name, Kind = DeviceKind.Headset, Via = Loc.T("via.24"),
                    Online = state.Online, Level = state.Level, Charging = state.Charging,
                    Detail = state.Detail != null ? Loc.T(state.Detail) : null,
                };
                if (!result.TryGetValue(reading.Id, out var other) || (reading.Online && !other.Online))
                    result[reading.Id] = reading;
            }
            return result.Values.ToList();
        }

        public TimeSpan Delay(IReadOnlyList<DeviceReading> last)
        {
            return last.Any(r => r.Online) ? TimeSpan.FromSeconds(60) : TimeSpan.FromSeconds(30);
        }

        static State Query(Spec spec, List<HidInfo> collections)
        {
            var candidates = collections
                .Where(i => spec.Interface < 0 || Math.Max(i.InterfaceNumber, 0) == spec.Interface)
                .Where(i => i.AccessDenied || (spec.Page == 0
                    ? i.UsagePage >= 0xFF00 || spec.Interface >= 0
                    : i.UsagePage == spec.Page && i.Usage == spec.Usage))
                .OrderBy(i => i.OutputLength > 0 ? 0 : 1)
                .ToList();
            if (candidates.Count == 0) return Off with { Detail = "d.noLink" };
            if (candidates.All(i => i.AccessDenied)) return Off with { Detail = "d.needsAdmin" };

            // A clean "headset off" from one collection beats an error from another that can't take the request
            State? clean = null;
            State error = Off;
            foreach (HidInfo info in candidates.Where(i => !i.AccessDenied))
            {
                try
                {
                    using var dev = new HidDevice(info);
                    State state = spec.Query(new Io(dev));
                    if (state.Online) return state;
                    clean ??= state;
                }
                catch (Exception e)
                {
                    error = Off with { Detail = Loc.T("d.error", e.Message) };
                }
            }
            return clean ?? error;
        }

        /// <summary>Status reports (B0) can also arrive unasked on this interface; other replies start with 20.</summary>
        static byte[]? NovaStatus(Io io) => io.Ask(new byte[] { 0x00, 0xB0 }, d => d.Length > 4 && d[0] == 0xB0);

        static State LogitechVoltage(Io io, byte featureIndex, byte function, (int Mv, int Percent)[] table)
        {
            var r = io.Ask(Pad(20, 0x11, 0xFF, featureIndex, function),
                d => d.Length >= 7 && d[0] == 0x11 && d[1] == 0xFF && ((d[2] == featureIndex && d[3] == function) || d[2] == 0xFF));
            if (r == null || r[2] == 0xFF) return Off; // HID++ error: headset off
            int mv = (r[4] << 8) | r[5];
            if (mv < table[^1].Mv) return Off;
            return On(Interpolate(mv, table), r[6] == 0x03);
        }

        static int Interpolate(int mv, (int Mv, int Percent)[] table)
        {
            if (mv >= table[0].Mv) return 100;
            for (int i = 1; i < table.Length; i++)
            {
                if (mv < table[i].Mv) continue;
                var (hiMv, hiP) = table[i - 1];
                var (loMv, loP) = table[i];
                return loP + (mv - loMv) * (hiP - loP) / (hiMv - loMv);
            }
            return 0;
        }

        /// <summary>Clamps v to a..b and scales it to c..d (integer arithmetic, as HeadsetControl's map()).</summary>
        static int Map(int v, int a, int b, int c, int d)
        {
            v = Math.Clamp(v, a, b);
            return c + (v - a) * (d - c) / (b - a);
        }

        static byte[] Pad(int length, params byte[] bytes)
        {
            var report = new byte[Math.Max(length, bytes.Length)];
            Array.Copy(bytes, report, bytes.Length);
            return report;
        }

        /// <summary>Request/response helpers over one HID collection, with hidapi's view of input reports.</summary>
        sealed class Io
        {
            readonly HidDevice dev;

            public Io(HidDevice dev)
            {
                this.dev = dev;
            }

            public ushort ProductId => dev.Info.ProductId;

            /// <summary>An input report as hidapi returns it: without the 0x00 Windows adds when there are no report IDs.</summary>
            public static byte[] Data(byte[] raw) => raw.Length > 0 && raw[0] == 0x00 ? raw[1..] : raw;

            public bool Send(byte[] report) => dev.Write(report, TimeoutMs);

            /// <summary>Sends a request and returns the first input report that <paramref name="match"/> accepts.</summary>
            public byte[]? Ask(byte[] request, Func<byte[], bool> match, int reads = 4)
            {
                dev.Flush();
                return Send(request) ? Receive(match, reads) : null;
            }

            public byte[]? Receive(Func<byte[], bool> match, int reads = 4)
            {
                DateTime deadline = DateTime.UtcNow.AddMilliseconds(TimeoutMs);
                for (int i = 0; i < reads && DateTime.UtcNow < deadline; i++)
                {
                    byte[]? raw = dev.Read(Math.Max((int)(deadline - DateTime.UtcNow).TotalMilliseconds, 1));
                    if (raw == null) return null;
                    byte[] data = Data(raw);
                    if (match(data)) return data;
                }
                return null;
            }

            /// <summary>Throws away whatever the device sent unasked.</summary>
            public void Drain()
            {
                for (int i = 0; i < 16 && dev.Read(5) != null; i++) { }
            }

            public bool SetFeature(byte[] report)
            {
                // Exactly the length the device declares, as hidapi does
                var buf = new byte[dev.Info.FeatureLength > 0 ? dev.Info.FeatureLength : report.Length];
                Array.Copy(report, buf, Math.Min(report.Length, buf.Length));
                return dev.SetFeature(buf);
            }

            public byte[]? GetFeature(byte reportId) => dev.Info.FeatureLength > 0 ? dev.GetFeature(reportId) : null;

            public byte[]? GetInputReport(byte reportId) => dev.Info.InputLength > 0 ? dev.GetInputReport(reportId) : null;
        }
    }
}
