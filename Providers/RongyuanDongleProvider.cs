using System;
using System.Collections.Generic;
using System.Threading;
using BatteryTray.Core;
using BatteryTray.Hid;

namespace BatteryTray.Providers
{
    /// <summary>
    /// 2.4 GHz receivers of the Rongyuan (RY) platform, used by Attack Shark, Royuan and many other brands. The
    /// receiver keeps the battery level of its paired mouse and keyboard, and one status command returns both:
    ///   SetFeature [00 F7 00 ...] on interface 2 (vendor page 0xFFFF, usage 2, 64-byte feature report), then
    ///   GetFeature -> [00 canRead kbBattery mouseBattery kbOffline mouseOffline canSend ...] (offline: 0 = connected).
    /// Taken from the official ATTACK SHARK web driver (qmk.top), which sends it every 5 s; verified on the
    /// Attack Shark R3 receiver 3151:402D. It is a read-only status query.
    /// On a USB cable the mouse leaves the receiver (it reports the mouse offline) and shows up as its own USB device,
    /// which charges it but has no battery query - the web driver doesn't show a level on the cable either (verified:
    /// the R3 on cable, 3151:4026, just echoes the status command). So a mouse on the cable is shown as charging, with
    /// the level it had before; the receiver reports the real one once the cable is out.
    /// </summary>
    public sealed class RongyuanDongleProvider : IBatteryProvider
    {
        const byte StatusCommand = 0xF7;

        /// <summary>(vid &lt;&lt; 16) | pid of every receiver the web driver drives with this protocol.</summary>
        static readonly HashSet<uint> Receivers = new()
        {
            0x0DB0197E, 0x0DB08E9A, 0x145F0376, 0x145F038D, 0x14A5C045, 0x25AA2008,
            0x25AA4008, 0x25AA4018, 0x31514011, 0x31514014, 0x31514017, 0x31514021,
            0x31514027, 0x3151402D, 0x31514038, 0x31515006, 0x31515007, 0x31515008,
            0x31515020, 0x31515026, 0x3151502B, 0x31515031, 0x31515037, 0x31515038,
            0x3151503D, 0x3151503F, 0x31515040, 0x31515042, 0x31515045, 0x3151504B,
            0x3151504E, 0x31515050, 0x31515052, 0x31515055, 0x3151505A, 0x342DE3E0,
            0x342DE3E1, 0x342DE3E2, 0x342DE3E6, 0x342DE3E7, 0x342DE3E8, 0x342DE3F5,
            0x342DE3F6, 0x342DE3F7, 0x342DE3FB, 0x342DE3FC, 0x342DE3FD, 0x347AA302,
            0x347AA305, 0x374AA205, 0x374AA217, 0x374AA220, 0x374AA223, 0x374AA226,
            0x374AA229, 0x374AA234, 0x374AA237, 0x374AA251, 0x374AA263, 0x374AA268,
            0x374AA311, 0x374AA314, 0x374AA319, 0x374AA322, 0x374AA329, 0x374AA355,
            0x379A1804, 0x379A1807, 0x38A90002, 0x38EE0000, 0x38EE0002, 0x38EE0039,
            0x391D1A04, 0x391D1A08, 0x391D1A0E, 0x391D1A13, 0x391D1A25, 0x391D1A2B,
            0x391D1A34, 0x391D2017, 0x391D2A05,
        };

        /// <summary>(vid &lt;&lt; 16) | pid of the mice the web driver drives on a USB cable.</summary>
        static readonly HashSet<uint> WiredMice = new()
        {
            0x3151400B, 0x31514026, 0x3151402A, 0x31515032, 0x31515035, 0x3151503A, 0x31515043, 0x31515058,
            0x39AB6001, 0x25AA4005, 0x25AA4006, 0x25AA4007, 0x25AA4019, 0x347AA301, 0x347AA304, 0x374AA310,
            0x374AA313, 0x374AA318, 0x374AA321, 0x374AA328, 0x374AA354, 0x391D1004, 0x391D100F, 0x391D1008,
            0x391D103A, 0x391D1015, 0x391D1028, 0x391D102F, 0x38A90019, 0x38A90021, 0x379A3357, 0x38EE0038,
        };

        /// <summary>Receivers known to belong to a mouse: their mouse slot is listed even while it's offline.</summary>
        static readonly Dictionary<uint, string> KnownMice = new()
        {
            [0x3151402D] = "Attack Shark R3",
        };

        readonly StateStore state;
        readonly HashSet<string> seenOnline = new();

        public RongyuanDongleProvider(StateStore state)
        {
            this.state = state;
        }

        public string Name => "Rongyuan 2.4G";

        static uint Key(ushort vid, ushort pid) => ((uint)vid << 16) | pid;

        public IReadOnlyList<DeviceReading> Poll()
        {
            var result = new List<DeviceReading>();
            var receiverMice = new List<int>(); // indexes in result of the receivers' mouse slots
            foreach (HidInfo info in HidBus.Enumerate((v, p) => Receivers.Contains(Key(v, p))))
            {
                if (info.InterfaceNumber != 2) continue;
                if (!info.AccessDenied && (info.UsagePage != 0xFFFF || info.Usage != 2 || info.FeatureLength < 7)) continue;

                uint key = Key(info.VendorId, info.ProductId);
                string baseId = $"ry:{info.VendorId:x4}:{info.ProductId:x4}";
                byte[]? status = null;
                string? error = null;
                if (info.AccessDenied)
                    error = "нужны права администратора";
                else
                {
                    try
                    {
                        status = QueryStatus(info);
                        if (status == null) error = "приёмник не ответил";
                    }
                    catch (Exception e)
                    {
                        error = "ошибка: " + e.Message;
                    }
                }

                bool knownMouse = KnownMice.TryGetValue(key, out string? mouseName);
                if (AddSlot(result, baseId + ":mouse", mouseName ?? info.Product ?? "Мышь", DeviceKind.Mouse,
                        status, levelAt: 3, offlineAt: 5, error, alwaysList: knownMouse))
                    receiverMice.Add(result.Count - 1);
                AddSlot(result, baseId + ":keyboard", "Клавиатура", DeviceKind.Keyboard,
                    status, levelAt: 2, offlineAt: 4, error, alwaysList: false);
            }
            AddWiredMice(result, receiverMice);
            return result;
        }

        /// <summary>A mouse on the cable is the receiver's mouse that went offline when the cable went in, so it takes
        /// over that slot. Without such a slot it is left out: the same IDs cover wired-only mice, with no battery.</summary>
        void AddWiredMice(List<DeviceReading> result, List<int> receiverMice)
        {
            foreach (HidInfo info in HidBus.Enumerate((v, p) => WiredMice.Contains(Key(v, p))))
            {
                if (info.InterfaceNumber != 2) continue;
                if (!info.AccessDenied && (info.UsagePage != 0xFFFF || info.Usage != 2)) continue;

                int slot = receiverMice.FindIndex(i => !result[i].Online);
                if (slot < 0) continue;
                int at = receiverMice[slot];
                receiverMice.RemoveAt(slot);
                DeviceReading offline = result[at];
                int? before = state.Get(offline.Id)?.Level;
                result[at] = offline with
                {
                    Via = "кабель", Online = true, Charging = true, Level = before,
                    Detail = before != null ? "уровень до подключения кабеля" : "по кабелю мышь не сообщает заряд",
                };
            }
        }

        public TimeSpan Delay(IReadOnlyList<DeviceReading> last)
        {
            foreach (var r in last)
                if (r.Online) return TimeSpan.FromSeconds(30);
            return TimeSpan.FromSeconds(15);
        }

        /// <summary>Adds the slot's reading; false when the slot isn't listed.</summary>
        bool AddSlot(List<DeviceReading> result, string id, string name, DeviceKind kind, byte[]? status,
            int levelAt, int offlineAt, string? error, bool alwaysList)
        {
            bool online = status != null && status[offlineAt] == 0;
            if (online) seenOnline.Add(id);
            // A receiver has a mouse and a keyboard slot; only list the ones that were ever used
            else if (!alwaysList && !seenOnline.Contains(id) && state.Get(id) == null) return false;

            // 0 right after wake-up means "not reported yet", not an empty battery
            int? level = online && status![levelAt] is > 0 and <= 100 ? status[levelAt] : null;
            // The receiver answering with "offline" is the normal state of a mouse that went to sleep
            string? detail = online ? (level == null ? "уровень пока неизвестен" : null) : error ?? "спит или выключена";
            result.Add(new DeviceReading
            {
                Id = id, Name = name, Kind = kind, Via = "2.4 ГГц", Online = online, Level = level, Detail = detail,
            });
            return true;
        }

        static byte[]? QueryStatus(HidInfo info)
        {
            using var dev = new HidDevice(info, overlapped: false);
            for (int attempt = 0; attempt < 3; attempt++)
            {
                if (attempt > 0) Thread.Sleep(100);
                var command = new byte[info.FeatureLength];
                command[1] = StatusCommand;
                if (!dev.SetFeature(command)) return null;
                Thread.Sleep(20);
                byte[]? reply = dev.GetFeature(0);
                // All zeros would mean both devices online at 0% - the receiver just hasn't filled the reply yet
                if (reply != null && reply.Length >= 7 && (reply[2] | reply[3] | reply[4] | reply[5] | reply[6]) != 0)
                    return reply;
            }
            return null;
        }
    }
}
