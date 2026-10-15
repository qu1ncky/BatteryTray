using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using BatteryTray.Core;
using BatteryTray.Hid;

namespace BatteryTray.Providers
{
    /// <summary>
    /// Razer wireless mice and keyboards, through their receiver or cable, with the protocol OpenRazer uses
    /// (github.com/openrazer/openrazer, driver/razercommon.c, razerchromacommon.c): a 90-byte feature report
    /// (report ID 0) on interface 0 (mice) or 2/3 (keyboards):
    ///   [0] status  [1] transaction id  [2..3] remaining packets  [4] protocol 0  [5] data size  [6] class  [7] id
    ///   [8..87] arguments  [88] crc = XOR of bytes 2..87  [89] reserved
    /// Battery: class 07 id 80 -> arguments[1] = 0..255; charging: class 07 id 84 -> arguments[1] = 0/1.
    /// Reply status 02 = ok, 01 = busy, 04 = timeout (mouse off), 05 = not supported.
    /// The transaction id and the wait between set and get differ per device; the table is OpenRazer's.
    /// Read-only; not yet verified on hardware by this project.
    /// </summary>
    public sealed class RazerPeripheralProvider : IBatteryProvider
    {
        const ushort RazerVid = 0x1532;
        const int ReportLength = 91; // report ID + 90 bytes

        sealed record Model(string Key, string Name, DeviceKind Kind, bool Wireless, byte Tid, int Interface, int WaitMs,
            bool QueryCharging = true);

        static Model M(string key, string name, bool wireless, byte tid, int waitUs, bool charging = true) =>
            new(key, "Razer " + name, DeviceKind.Mouse, wireless, tid, 0, (waitUs + 999) / 1000, charging);

        static Model K(string key, string name, bool wireless, byte tid, int iface, int waitUs) =>
            new(key, "Razer " + name, DeviceKind.Keyboard, wireless, tid, iface, (waitUs + 999) / 1000);

        // "charging: false" = the device runs on AA batteries; OpenRazer never asks it for a charging state
        static readonly Dictionary<ushort, Model> Models = new()
        {
            [0x001F] = M("naga-epic", "Naga Epic", true, 0xFF, 600),
            [0x0024] = M("mamba-2012", "Mamba 2012", false, 0xFF, 600),
            [0x0025] = M("mamba-2012", "Mamba 2012", true, 0xFF, 600),
            [0x0032] = M("ouroboros", "Ouroboros", true, 0xFF, 600),
            [0x003E] = M("naga-epic-chroma", "Naga Epic Chroma", true, 0xFF, 600),
            [0x003F] = M("naga-epic-chroma", "Naga Epic Chroma", true, 0xFF, 600),
            [0x0044] = M("mamba", "Mamba", false, 0xFF, 600),
            [0x0045] = M("mamba", "Mamba", true, 0xFF, 600),
            [0x0059] = M("lancehead", "Lancehead", false, 0x3F, 600),
            [0x005A] = M("lancehead", "Lancehead", true, 0x3F, 600),
            [0x0062] = M("atheris", "Atheris", true, 0x1F, 400000, charging: false),
            [0x006F] = M("lancehead-wireless", "Lancehead Wireless", true, 0x1F, 31000),
            [0x0070] = M("lancehead-wireless", "Lancehead Wireless", false, 0x1F, 31000),
            [0x0072] = M("mamba-wireless", "Mamba Wireless", true, 0x3F, 31000),
            [0x0073] = M("mamba-wireless", "Mamba Wireless", false, 0x3F, 31000),
            [0x0077] = M("pro-click", "Pro Click", true, 0x1F, 31000),
            [0x0080] = M("pro-click", "Pro Click", false, 0x1F, 31000),
            [0x007A] = M("viper-ultimate", "Viper Ultimate", false, 0xFF, 59900),
            [0x007B] = M("viper-ultimate", "Viper Ultimate", true, 0xFF, 59900),
            [0x007C] = M("deathadder-v2-pro", "DeathAdder V2 Pro", false, 0x3F, 59900),
            [0x007D] = M("deathadder-v2-pro", "DeathAdder V2 Pro", true, 0x3F, 59900),
            [0x0083] = M("basilisk-x-hyperspeed", "Basilisk X HyperSpeed", true, 0xFF, 31000, charging: false),
            [0x0086] = M("basilisk-ultimate", "Basilisk Ultimate", false, 0x1F, 31000),
            [0x0088] = M("basilisk-ultimate", "Basilisk Ultimate", true, 0x1F, 31000),
            [0x008F] = M("naga-pro", "Naga Pro", false, 0x1F, 600),
            [0x0090] = M("naga-pro", "Naga Pro", true, 0x1F, 31000),
            [0x0094] = M("orochi-v2", "Orochi V2", true, 0x1F, 400000, charging: false),
            [0x0095] = M("orochi-v2", "Orochi V2", true, 0x1F, 400000, charging: false),
            [0x009A] = M("pro-click-mini", "Pro Click Mini", true, 0x1F, 31000),
            [0x009C] = M("deathadder-v2-x", "DeathAdder V2 X HyperSpeed", true, 0x1F, 31000, charging: false),
            [0x009E] = M("viper-mini-se", "Viper Mini SE", false, 0x1F, 59900),
            [0x009F] = M("viper-mini-se", "Viper Mini SE", true, 0x1F, 59900),
            [0x00A5] = M("viper-v2-pro", "Viper V2 Pro", false, 0x1F, 31000),
            [0x00A6] = M("viper-v2-pro", "Viper V2 Pro", true, 0x1F, 31000),
            [0x00A7] = M("naga-v2-pro", "Naga V2 Pro", false, 0x1F, 31000),
            [0x00A8] = M("naga-v2-pro", "Naga V2 Pro", true, 0x1F, 31000),
            [0x00AA] = M("basilisk-v3-pro", "Basilisk V3 Pro", false, 0x1F, 31000),
            [0x00AB] = M("basilisk-v3-pro", "Basilisk V3 Pro", true, 0x1F, 31000),
            [0x00AF] = M("cobra-pro", "Cobra Pro", false, 0x1F, 31000),
            [0x00B0] = M("cobra-pro", "Cobra Pro", true, 0x1F, 31000),
            [0x00B3] = M("hyperpolling", "мышь (HyperPolling)", true, 0x1F, 59900),
            [0x00B4] = M("naga-v2-hyperspeed", "Naga V2 HyperSpeed", true, 0x1F, 31000, charging: false),
            [0x00B6] = M("deathadder-v3-pro", "DeathAdder V3 Pro", false, 0x1F, 31000),
            [0x00B7] = M("deathadder-v3-pro", "DeathAdder V3 Pro", true, 0x1F, 31000),
            [0x00C2] = M("deathadder-v3-pro", "DeathAdder V3 Pro", false, 0x1F, 31000),
            [0x00C3] = M("deathadder-v3-pro", "DeathAdder V3 Pro", true, 0x1F, 31000),
            [0x00B8] = M("viper-v3-hyperspeed", "Viper V3 HyperSpeed", true, 0x1F, 59900, charging: false),
            [0x00B9] = M("basilisk-v3-x", "Basilisk V3 X HyperSpeed", true, 0x1F, 31000, charging: false),
            [0x00BE] = M("deathadder-v4-pro", "DeathAdder V4 Pro", false, 0x1F, 31000),
            [0x00BF] = M("deathadder-v4-pro", "DeathAdder V4 Pro", true, 0x1F, 31000),
            [0x00C0] = M("viper-v3-pro", "Viper V3 Pro", false, 0x1F, 31000),
            [0x00C1] = M("viper-v3-pro", "Viper V3 Pro", true, 0x1F, 59900),
            [0x00C4] = M("deathadder-v3-hyperspeed", "DeathAdder V3 HyperSpeed", false, 0x1F, 31000),
            [0x00C5] = M("deathadder-v3-hyperspeed", "DeathAdder V3 HyperSpeed", true, 0x1F, 31000),
            [0x00C7] = M("pro-click-v2-vertical", "Pro Click V2 Vertical", false, 0x1F, 31000),
            [0x00C8] = M("pro-click-v2-vertical", "Pro Click V2 Vertical", true, 0x1F, 31000),
            [0x00CC] = M("basilisk-v3-pro-35k", "Basilisk V3 Pro 35K", false, 0x1F, 31000),
            [0x00CD] = M("basilisk-v3-pro-35k", "Basilisk V3 Pro 35K", true, 0x1F, 31000),
            [0x00D0] = M("pro-click-v2", "Pro Click V2", false, 0x1F, 31000),
            [0x00D1] = M("pro-click-v2", "Pro Click V2", true, 0x1F, 31000),
            [0x00D3] = M("basilisk-mobile", "Basilisk Mobile", false, 0x1F, 31000, charging: false),
            [0x00D4] = M("basilisk-mobile", "Basilisk Mobile", true, 0x1F, 31000, charging: false),
            [0x00D6] = M("basilisk-v3-pro-35k-pg", "Basilisk V3 Pro 35K Phantom Green", false, 0x1F, 31000),
            [0x00D7] = M("basilisk-v3-pro-35k-pg", "Basilisk V3 Pro 35K Phantom Green", true, 0x1F, 31000),
            [0x00DA] = M("cobra-hyperspeed", "Cobra HyperSpeed", false, 0x1F, 31000),
            [0x00DB] = M("cobra-hyperspeed", "Cobra HyperSpeed", true, 0x1F, 31000),
            [0x00DE] = M("viper-v3-pro-se", "Viper V3 Pro SE", false, 0x1F, 31000),
            [0x00DF] = M("viper-v3-pro-se", "Viper V3 Pro SE", true, 0x1F, 59900),

            [0x0258] = K("blackwidow-v3-mini", "BlackWidow V3 Mini HyperSpeed", false, 0x1F, 3, 600),
            [0x0271] = K("blackwidow-v3-mini", "BlackWidow V3 Mini HyperSpeed", true, 0x9F, 3, 4900),
            [0x025A] = K("blackwidow-v3-pro", "BlackWidow V3 Pro", false, 0x3F, 2, 600),
            [0x025C] = K("blackwidow-v3-pro", "BlackWidow V3 Pro", true, 0x9F, 2, 4900),
            [0x0292] = K("deathstalker-v2-pro", "DeathStalker V2 Pro", false, 0x1F, 3, 600),
            [0x0290] = K("deathstalker-v2-pro", "DeathStalker V2 Pro", true, 0x9F, 2, 4900),
            [0x0298] = K("deathstalker-v2-pro-tkl", "DeathStalker V2 Pro TKL", false, 0x1F, 3, 600),
            [0x0296] = K("deathstalker-v2-pro-tkl", "DeathStalker V2 Pro TKL", true, 0x9F, 2, 4900),
            [0x02B9] = K("blackwidow-v4-mini", "BlackWidow V4 Mini HyperSpeed", false, 0x1F, 3, 600),
            [0x02BA] = K("blackwidow-v4-mini", "BlackWidow V4 Mini HyperSpeed", true, 0x9F, 3, 4900),
            [0x02D7] = K("blackwidow-v4-tkl", "BlackWidow V4 TKL HyperSpeed", false, 0x1F, 3, 600),
            [0x02D5] = K("blackwidow-v4-tkl", "BlackWidow V4 TKL HyperSpeed", true, 0x9F, 2, 4900),
            [0x02CD] = K("joro", "Joro", true, 0x1F, 3, 600),
        };

        public string Name => "Razer";

        public IReadOnlyList<DeviceReading> Poll()
        {
            // A mouse on its cable and on its receiver at once shows up twice: keep the reading that has a level
            var result = new Dictionary<string, DeviceReading>();
            var found = HidBus.Enumerate((v, p) => v == RazerVid && Models.ContainsKey(p));
            foreach (var group in found.GroupBy(i => i.ProductId))
            {
                Model model = Models[group.Key];
                var candidates = group.Where(i => Math.Max(i.InterfaceNumber, 0) == model.Interface).ToList();
                DeviceReading reading = Query(model, candidates);
                if (!result.TryGetValue(reading.Id, out var other) || (reading.Level != null && other.Level == null))
                    result[reading.Id] = reading;
            }
            return result.Values.ToList();
        }

        public TimeSpan Delay(IReadOnlyList<DeviceReading> last)
        {
            return last.Any(r => r.Online) ? TimeSpan.FromSeconds(60) : TimeSpan.FromSeconds(20);
        }

        static DeviceReading Query(Model model, List<HidInfo> candidates)
        {
            var reading = new DeviceReading
            {
                Id = "razer:" + model.Key, Name = model.Name, Kind = model.Kind,
                Via = model.Wireless ? "2.4 ГГц" : "кабель", Online = false, Detail = "выключена или спит",
            };
            foreach (HidInfo info in candidates)
            {
                if (info.AccessDenied)
                {
                    reading = reading with { Detail = "нужны права администратора" };
                    continue;
                }
                if (info.FeatureLength != ReportLength) continue;
                try
                {
                    using var dev = new HidDevice(info, featuresOnly: true);
                    byte[]? level = Command(dev, model, 0x07, 0x80);
                    if (level == null) continue;
                    int percent = (int)Math.Round(level[1] * 100 / 255.0);
                    if (percent == 0) continue; // OpenRazer treats 0 as a bogus reading from a sleeping mouse
                    bool charging = model.QueryCharging && Command(dev, model, 0x07, 0x84) is byte[] c && c[1] != 0;
                    return reading with { Online = true, Level = percent, Charging = charging, Detail = null };
                }
                catch (Exception e)
                {
                    reading = reading with { Detail = "ошибка: " + e.Message };
                }
            }
            return reading;
        }

        /// <summary>Sends one command and returns its reply arguments, or null (no reply, timeout, not supported).</summary>
        static byte[]? Command(HidDevice dev, Model model, byte commandClass, byte commandId)
        {
            for (int attempt = 0; attempt < 3; attempt++)
            {
                if (attempt > 0) Thread.Sleep(10);
                var request = new byte[ReportLength];
                request[2] = model.Tid;
                request[6] = 0x02; // data size
                request[7] = commandClass;
                request[8] = commandId;
                byte crc = 0;
                for (int i = 3; i <= 88; i++) crc ^= request[i]; // report bytes 2..87
                request[89] = crc;
                if (!dev.SetFeature(request)) return null;

                Thread.Sleep(Math.Max(model.WaitMs, 1));
                byte[]? reply = dev.GetFeature(0);
                if (reply == null || reply.Length < ReportLength) return null;
                byte status = reply[1];
                if (reply[7] != commandClass || reply[8] != commandId) continue; // someone else's reply (Synapse)
                // OpenRazer: "some commands respond with 'busy' but succeed" - busy counts as success
                if (status is 0x02 or 0x01) return reply[9..89];
                return null; // failure, timeout (device off) or not supported
            }
            return null;
        }
    }
}
