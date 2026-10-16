// Razer Barracuda X (2022) over its 2.4 GHz dongle.
// Protocol, framing and charge detection ported from github.com/mehdibouchami/razer-barracuda-x-battery
// (MIT License, Copyright (c) 2026 Mehdi Bouchami).
using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using BatteryTray.Core;
using BatteryTray.Hid;

namespace BatteryTray.Providers
{
    public sealed class RazerBarracudaProvider : IBatteryProvider, IPushingProvider
    {
        const string Id = "razer:barracuda-x";
        const string DeviceName = "Razer Barracuda X";
        static string NeedsAdmin => Loc.T("d.needsAdmin");
        const int NotChargingChecks = 3; // polls in a row without charging before the level may move again
        const int ChargerRiseMv = 15;    // a charger lifts the voltage at least this much above the resting one
        const int HoldMv = 4195;         // the charger's last stage holds the battery at exactly 4.20 V
        static readonly TimeSpan SettleQuiet = TimeSpan.FromSeconds(50), SettleMax = TimeSpan.FromMinutes(5);

        readonly StateStore state;
        int shownPercent;
        bool chargingSticky;
        int notChargingStreak;
        int? restingMv; // median voltage of the last poll on battery; unknown after the headset was away
        HeadsetEvents? events;
        bool seeded;

        // The current stay on the cable, as the voltage shows it
        DateTime? cableSince;
        int cableStartMv;
        bool sawHold, full;
        int? levelBeforeCable, lastLevel;
        DateTime? unpluggedAt;
        bool settling;

        public RazerBarracudaProvider(StateStore state)
        {
            this.state = state;
            shownPercent = state.Get(Id)?.Level ?? -1;
        }

        public string Name => DeviceName;

        public event Action? Pushed;

        public IReadOnlyList<DeviceReading> Poll()
        {
            List<HidInfo> all = HidBus.Enumerate((v, p) => v == BarracudaProtocol.RazerVid && BarracudaProtocol.DonglePids.Contains(p));
            if (all.Count == 0)
            {
                // Dongle unplugged
                events?.Dispose();
                events = null;
                return Array.Empty<DeviceReading>();
            }

            List<HidInfo> candidates = BarracudaProtocol.ControlInterfaces(all);
            if (candidates.Count == 0)
                return new[] { Offline(all.Any(i => i.AccessDenied) ? NeedsAdmin : Loc.T("d.receiverSilent")) };

            if (events == null || !events.Running || events.Path != candidates[0].Path)
            {
                events?.Dispose();
                events = new HeadsetEvents(candidates[0], () => Pushed?.Invoke());
                // The headset only sends its level when it changes. After a quick restart of the app (an update)
                // the number it sent just before is still right, and better than the voltage estimate.
                if (!seeded && state.Get(Id) is { Estimate: false, Charging: false, Level: int last } before
                    && DateTime.Now - before.Time < TimeSpan.FromMinutes(2))
                    events.Seed(last);
                seeded = true;
            }
            HeadsetEvents.Snapshot ev = events.Current();

            // On battery with the headset's own level known, a couple of samples just confirm it's on; on the cable
            // the voltage shows the charger's stage, so take the full set
            BatteryReading? r = BarracudaProtocol.Read(candidates,
                ev.Percent != null && ev.Charger == false ? 2 : BarracudaProtocol.ChargeSamples, out string? status);
            if (r == null)
            {
                restingMv = null;
                return new[] { Offline(status == "headset off or out of range" ? null : status) };
            }

            bool onCable;
            if (ev.Charger is bool plugged)
            {
                onCable = plugged;
                chargingSticky = plugged;
                notChargingStreak = 0;
                if (!plugged) restingMv = r.Median;
            }
            else
                onCable = IsCharging(r); // until the headset has said, the voltage tells whether a charger is on

            DeviceReading reading = onCable ? OnCable(r) : OnBattery(r, ev);
            return new[] { reading };
        }

        /// <summary>
        /// On the cable no level is shown: the voltage is the charger's, and the headset's own number is no better -
        /// it just climbs 1% per 20 s check towards the 100% that 4.20 V means (a charge that takes over an hour for
        /// the last quarter). What the voltage does show is the charger's stage: rising, then held at exactly 4.20 V
        /// for the last stretch, then let go once the battery is full - that is "charged".
        /// </summary>
        DeviceReading OnCable(BatteryReading r)
        {
            settling = false;
            if (cableSince == null)
            {
                cableSince = DateTime.Now;
                cableStartMv = r.Millivolts;
                sawHold = full = false;
                levelBeforeCable = lastLevel;
            }
            bool hold = r.Millivolts >= HoldMv;
            if (hold)
            {
                sawHold = true;
                full = false; // a top-up of a full battery starts with a new hold
            }
            else if (sawHold && r.Millivolts < BarracudaProtocol.ChargeVoltageMv)
                full = true;
            else if (!sawHold && DateTime.Now - cableSince > TimeSpan.FromMinutes(10) && r.Millivolts >= 4100
                     && r.Millivolts - cableStartMv < 10)
                full = true; // plugged in already full: the charger never started, the voltage just sits there

            if (full)
            {
                levelBeforeCable = 100; // a top-up from here starts full
                return Online(100, false, charging: false, full: true, Loc.T("d.unplugOk"));
            }
            string stage = Loc.T(hold ? "d.almostFull" : "d.cableHidden");
            return Online(null, false, charging: true, full: false,
                levelBeforeCable is int before ? $"{stage} · {Loc.T("d.wasBefore", before)}" : stage);
        }

        /// <summary>
        /// On battery: the headset's own number, which it sends whenever it moves. Right after the cable comes out
        /// that number slides down 1% per 20 s check while the voltage settles (92, 91, 90, 89, then quiet), so it
        /// is held back until it has been still for a while.
        /// </summary>
        DeviceReading OnBattery(BatteryReading r, HeadsetEvents.Snapshot ev)
        {
            if (cableSince != null)
            {
                cableSince = null;
                unpluggedAt = DateTime.Now;
            }
            DateTime now = DateTime.Now;
            settling = unpluggedAt is DateTime u && now - u < SettleMax
                       && (now - u < SettleQuiet || now - ev.PercentTime < SettleQuiet);
            if (settling)
                return Online(null, false, charging: false, full: false, Loc.T("d.settling"));
            unpluggedAt = null;

            if (ev.Percent is int own)
            {
                shownPercent = own; // a later voltage estimate starts from the headset's number
                lastLevel = own;
                return Online(own, false, charging: false, full: false, null);
            }
            int level = Step(r.Percent);
            lastLevel = level;
            return Online(level, true, charging: false, full: false, Loc.T("d.estimate"));
        }

        static DeviceReading Online(int? level, bool estimate, bool charging, bool full, string? detail)
        {
            return new DeviceReading
            {
                Id = Id, Name = DeviceName, Kind = DeviceKind.Headset, Via = Loc.T("via.24"), Online = true,
                Level = level, Estimate = estimate, Charging = charging, FullOnCharger = full, Detail = detail,
            };
        }

        public TimeSpan Delay(IReadOnlyList<DeviceReading> last)
        {
            if (last.Count == 0) return TimeSpan.FromSeconds(15);
            if (last[0].Online) return TimeSpan.FromSeconds(settling ? 15 : 60);
            // Headset off or on Bluetooth: look again soon. No access won't fix itself, so check rarely.
            return last[0].Detail == NeedsAdmin ? TimeSpan.FromMinutes(5) : TimeSpan.FromSeconds(15);
        }

        static DeviceReading Offline(string? detail)
        {
            return new DeviceReading
            {
                Id = Id, Name = DeviceName, Kind = DeviceKind.Headset, Via = Loc.T("via.24"), Online = false, Detail = detail,
            };
        }

        /// <summary>
        /// Fallback until the headset has reported the charger itself. Charging starts when the samples swing and the
        /// voltage also rose above the resting one: a load or a glitch can make the samples swing, but only a charger
        /// lifts the voltage. (With no resting voltage yet - right after start or after the headset was away - the
        /// swing alone has to do.) Near full the charger holds a flat 4.20 V, caught by the voltage threshold.
        /// Charging detection can miss a check (the voltage swing isn't always visible in one sample window), and a
        /// single miss would let the charger-inflated voltage update the level. So charging sticks until several
        /// checks in a row say otherwise.
        /// </summary>
        bool IsCharging(BatteryReading r)
        {
            bool swing = r.Charging && (chargingSticky || restingMv == null || r.Millivolts >= BarracudaProtocol.ChargeVoltageMv
                || r.Median >= restingMv + ChargerRiseMv);
            bool charging;
            if (swing)
            {
                chargingSticky = true;
                notChargingStreak = 0;
                charging = true;
            }
            else if (chargingSticky && ++notChargingStreak < NotChargingChecks)
                charging = true;
            else
            {
                chargingSticky = false;
                charging = false;
            }
            if (r.Charging && !charging)
                Log.Write($"Barracuda: swing without a rise ignored, samples {string.Join(",", r.Samples)}, resting {restingMv} mV");
            if (!charging) restingMv = r.Median;
            return charging;
        }

        /// <summary>
        /// Rounds to 5% steps. Hysteresis: the shown step only changes once the estimate is 4+ points away from it,
        /// so voltage noise doesn't make it flicker.
        /// </summary>
        int Step(int percent)
        {
            if (shownPercent < 0 || Math.Abs(percent - shownPercent) >= 4)
                shownPercent = (percent + 2) / 5 * 5;
            return shownPercent;
        }
    }

    /// <summary>
    /// What the headset reports by itself through the dongle, found by listening to the dongle while plugging the
    /// cable in and out and switching the headset off and on. Notification frames (RACE frame type 0x08) with the
    /// payload [id 02 01 value]:
    ///   0x20 - headset link: 01 connected (switched on), 00 lost (switched off); the moment it happens;
    ///   0x21 - the headset's own battery percentage, and right after it
    ///   0x2A - the charger: 01 plugged in, 00 unplugged.
    /// The headset checks about every 20 s and sends the pair when the level differs from the one it sent last: the
    /// cable going in, every 20 s while charging, 92, 91, 90... on battery. Switched off and on again it stays quiet
    /// if the level is unchanged (seen: 88% before, nothing after) and sends it if it moved meanwhile (seen: 92 before,
    /// 91 ~17 s after) - so the last level heard stays right across a power cycle.
    /// The headset's number beats the voltage estimate (it said 92% where the voltage curve said 96%) and keeps
    /// counting while charging, when the voltage is useless. Passive: it only reads, on its own handle.
    /// </summary>
    sealed class HeadsetEvents : IDisposable
    {
        const byte NotificationFrame = 0x08, LinkEvent = 0x20, BatteryEvent = 0x21, ChargerEvent = 0x2A;

        readonly object gate = new();
        readonly HidInfo info;
        readonly Action changed;
        volatile bool stopped;
        int? percent;
        bool? charger;
        DateTime percentTime, chargerChanged;

        public HeadsetEvents(HidInfo info, Action changed)
        {
            this.info = info;
            this.changed = changed;
            new Thread(Listen) { IsBackground = true, Name = "Barracuda events" }.Start();
        }

        public string Path => info.Path;
        public bool Running { get; private set; } = true;

        /// <param name="PercentTime">When the level last changed (the headset only sends changes).</param>
        public readonly record struct Snapshot(int? Percent, DateTime PercentTime, bool? Charger, DateTime ChargerChanged);

        public Snapshot Current()
        {
            lock (gate) return new Snapshot(percent, percentTime, charger, chargerChanged);
        }

        /// <summary>A level remembered from just before a restart of the app, used until the headset sends one.</summary>
        public void Seed(int level)
        {
            lock (gate)
            {
                percent ??= level;
            }
        }

        /// <summary>Stops within a second. The listening thread owns the handle and closes it itself, so no read
        /// is ever left pending on a closed handle.</summary>
        public void Dispose()
        {
            stopped = true;
        }

        void Listen()
        {
            try
            {
                using var dev = new HidDevice(info, readOnly: true);
                byte reportId = BarracudaProtocol.ReportId(info);
                while (!stopped)
                {
                    byte[]? r = dev.Read(1000);
                    if (r != null) Process(r, reportId);
                }
            }
            catch (Exception e)
            {
                if (!stopped) Log.Write("Barracuda events: " + e.Message);
            }
            finally
            {
                Running = false;
            }
        }

        /// <summary>Takes the notifications out of one input report of the dongle.</summary>
        internal void Process(byte[] report, byte reportId)
        {
            foreach (byte[] f in BarracudaProtocol.Frames(report, reportId))
                if (f.Length >= 14 && f[2] == NotificationFrame && f[11] == 0x02) Handle(f[10], f[13]);
        }

        void Handle(byte id, byte value)
        {
            bool news;
            lock (gate)
            {
                if (id == LinkEvent && value <= 1)
                {
                    // The level stays (see above), but the cable may have come out while the headset was off and the
                    // charger is only reported along with a changed level: back to the voltage for that
                    news = true;
                    charger = null;
                    Log.Write($"Barracuda: headset {(value == 1 ? "connected" : "disconnected")}");
                }
                else if (id == BatteryEvent && value <= 100)
                {
                    news = percent != value;
                    percent = value;
                    if (news) percentTime = DateTime.Now;
                }
                else if (id == ChargerEvent && value <= 1)
                {
                    news = charger != (value == 1);
                    charger = value == 1;
                    if (news) chargerChanged = DateTime.Now;
                    if (news) Log.Write($"Barracuda: headset reports the cable {(value == 1 ? "plugged in" : "unplugged")}");
                }
                else return;
            }
            if (news) changed();
        }
    }

    public sealed class BatteryReading
    {
        /// <summary>Lowest sample: a charger lifts every sample, so the minimum is closest to the resting voltage.</summary>
        public int Millivolts;
        public int Percent;
        /// <summary>The samples swing like on a charger, or the voltage is above any resting voltage.</summary>
        public bool Charging;
        public int Median;
        public int[] Samples = Array.Empty<int>();
    }

    /// <summary>
    /// The headset is an Airoha chip; the dongle bridges USB HID to it. We send the Airoha "RACE" command
    /// GET_BATTERY (50 41 06 seq 01 00 31) and the reply carries the battery voltage in millivolts.
    /// The dongle must be switched to "remote mode" (50 41 0E seq 02 E1 01) first so it forwards the command to
    /// the headset; we switch back to local mode afterwards (E1 00), as Razer's tool does.
    /// Two dongle bridge chips exist, with different HID framing (identified by the vendor collection's report ID):
    ///   0x02 - YS-Tech bridge (PID 0x0550): [02 80 len crcLo crcHi payload...], CRC-16/XMODEM over the whole
    ///          64-byte report; the write is acknowledged via GetInputReport ('O' ok / 'F' busy, retried);
    ///          replies arrive as input reports [02 len payload...].
    ///   0x01 - Macronix bridge (PID 0x0552): [01 80 len payload...] after an "app mode" handshake; replies are
    ///          [01 80 len payload...].
    /// Only read-only status commands are sent; settings and firmware are never written.
    /// </summary>
    public static class BarracudaProtocol
    {
        public const ushort RazerVid = 0x1532;
        public static readonly ushort[] DonglePids = { 0x0550, 0x0552 };
        const ushort VendorUsagePage = 0xFF00;
        const int TimeoutMs = 800;
        const byte RaceGetBattery = 0x31;
        public const int ChargeSamples = 7; // spread over ~2 s: consecutive samples can match while charging
        const int ChargeJitterMv = 12;    // spread across samples: ~3 mV on battery, 30-45 mV while charging
        public const int ChargeVoltageMv = 4180; // above any resting voltage, so the charger must be connected

        // Battery voltage (mV) for 0%, 10%, ... 100%: typical Li-ion discharge curve, checked against the Razer
        // mobile app (3975 mV = 70%) and against the headset's own number at the top (4113 mV = 87%, 4128 mV = 89%).
        static readonly int[] Curve = { 3300, 3680, 3750, 3790, 3830, 3870, 3910, 3960, 4050, 4135, 4190 };

        static byte sequence;

        /// <summary>Optional hook that receives every raw packet sent (">") and received ("&lt;").</summary>
        public static Action<string, byte[]>? Trace;

        /// <summary>Vendor interfaces that can carry the command, most likely first (the bridge uses 64-byte
        /// reports). Writing to the wrong one fails with ERROR_INVALID_PARAMETER.</summary>
        public static List<HidInfo> ControlInterfaces(IEnumerable<HidInfo> all)
        {
            return all
                .Where(i => !i.AccessDenied && i.UsagePage == VendorUsagePage && i.OutputLength > 0 && i.InputLength > 0)
                .OrderBy(i => i.InputLength == 64 && i.OutputLength == 64 ? 0 : 1)
                .ToList();
        }

        /// <summary>Tries each candidate interface until one answers. <paramref name="status"/> is null on success,
        /// otherwise says why there is no reading (headset away, or the last I/O error).</summary>
        public static BatteryReading? Read(List<HidInfo> candidates, int samples, out string? status)
        {
            string? error = null;
            bool answered = false;
            foreach (HidInfo info in candidates)
            {
                try
                {
                    using var dev = new HidDevice(info);
                    BatteryReading? reading = Query(dev, samples);
                    if (reading != null)
                    {
                        status = null;
                        return reading;
                    }
                    answered = true;
                }
                catch (Exception e)
                {
                    error = e.Message;
                    Trace?.Invoke("! " + e.Message, Array.Empty<byte>());
                }
            }
            // An interface that talked without error means the dongle works and the headset is just away
            status = answered || error == null ? "headset off or out of range" : Loc.T("d.error", error);
            return null;
        }

        /// <summary>Report ID of the vendor collection: 0x02 = YS-Tech bridge, otherwise Macronix. Falls back to the
        /// product ID when the descriptor couldn't be read.</summary>
        public static byte ReportId(HidInfo info)
        {
            if (info.OutputReportId != 0) return info.OutputReportId;
            return (byte)(info.ProductId == 0x0550 ? 0x02 : 0x01);
        }

        /// <summary>Returns null when the dongle is present but the headset doesn't answer (e.g. powered off).</summary>
        static BatteryReading? Query(HidDevice dev, int sampleCount)
        {
            dev.Flush();
            if (ReportId(dev.Info) != 0x02 && !EnterMacronixAppMode(dev)) return null;

            // Remote mode makes the dongle forward commands to the headset; in local mode the dongle
            // answers GET_BATTERY itself with a meaningless value.
            byte[]? reply = Race(dev, 0x0E, 0x02, 0xE1, 0x01);
            if (reply == null || reply[12] != 0x00) return null;

            // The headset reports voltage only - no charging flag and no percentage. While charging, the voltage is
            // raised by the charger and swings by tens of millivolts between samples; on battery it is steady within
            // a few millivolts. So take a few samples and use their spread to tell the two apart.
            var samples = new List<int>();
            try
            {
                for (int i = 0; i < sampleCount; i++)
                {
                    if (i > 0) Thread.Sleep(350);
                    reply = Race(dev, 0x06, 0x01, 0x00, RaceGetBattery);
                    // Response: [12]=status, [13..14]=millivolts (LE)
                    if (reply == null || reply.Length < 15 || reply[12] != 0x00) continue;
                    int sample = reply[13] | (reply[14] << 8);
                    if (sample >= 2500 && sample <= 4500) samples.Add(sample);
                }
            }
            finally
            {
                Race(dev, 0x0E, 0x02, 0xE1, 0x00);
            }
            if (samples.Count == 0) return null;

            // On battery the samples sit within 3-6 mV (one or two ADC steps). A single stray sample - seen once
            // while a USB device was being plugged in - must not count as a swing, so with enough samples the
            // lowest and the highest one are left out.
            var sorted = samples.OrderBy(s => s).ToList();
            int trim = sorted.Count >= 5 ? 1 : 0;
            int low = sorted[trim], high = sorted[^(1 + trim)];
            bool charging = high - low >= ChargeJitterMv || low >= ChargeVoltageMv;
            return new BatteryReading
            {
                Millivolts = low, Percent = ToPercent(low), Charging = charging, Median = sorted[sorted.Count / 2],
                Samples = samples.ToArray(),
            };
        }

        /// <summary>Sends an Airoha RACE command (50 41 group seq body...) and returns the matching
        /// response payload (50 49 ... [10]=group, [11]=0x80|seq, [12]=status ...), or null.</summary>
        static byte[]? Race(HidDevice dev, byte group, params byte[] body)
        {
            byte seq = NextSequence();
            var race = new byte[4 + body.Length];
            race[0] = 0x50;
            race[1] = 0x41;
            race[2] = group;
            race[3] = seq;
            Array.Copy(body, 0, race, 4, body.Length);
            bool Matches(byte[] p) => p.Length >= 13 && p[0] == 0x50 && p[1] == 0x49 && p[10] == group && p[11] == (0x80 | seq);
            return ReportId(dev.Info) == 0x02 ? SendYsTech(dev, race, Matches) : SendMacronix(dev, race, Matches);
        }

        static byte NextSequence()
        {
            sequence = (byte)(sequence % 127 + 1); // 1..127, as Razer's tool does
            return sequence;
        }

        // ---- YS-Tech bridge (report ID 0x02) ----

        static byte[]? SendYsTech(HidDevice dev, byte[] race, Func<byte[], bool> matches)
        {
            var frame = new byte[dev.Info.OutputLength];
            frame[0] = 0x02;
            frame[1] = 0x80;
            frame[2] = (byte)race.Length;
            Array.Copy(race, 0, frame, 5, race.Length);
            ushort crc = Crc16Xmodem(frame);
            frame[3] = (byte)crc;
            frame[4] = (byte)(crc >> 8);

            bool accepted = false;
            for (int attempt = 0; attempt < 5 && !accepted; attempt++)
            {
                if (attempt > 0) Thread.Sleep(50);
                Trace?.Invoke(">", frame.Take(5 + race.Length).ToArray());
                if (!dev.Write(frame, TimeoutMs)) return null;
                byte[]? status = dev.GetInputReport(0x02);
                if (status == null) return null;
                if (status[1] == (byte)'O') accepted = true;
                else if (status[1] != (byte)'F') return null;
            }
            if (!accepted) return null;

            return ReadUntil(dev, r =>
            {
                if (r.Length < 2 || r[0] != 0x02 || r[1] > r.Length - 2) return null;
                return FindFrame(r, 2, r[1], matches);
            });
        }

        static ushort Crc16Xmodem(byte[] data)
        {
            ushort crc = 0;
            foreach (byte b in data)
            {
                crc ^= (ushort)(b << 8);
                for (int i = 0; i < 8; i++)
                    crc = (ushort)((crc & 0x8000) != 0 ? (crc << 1) ^ 0x1021 : crc << 1);
            }
            return crc;
        }

        // ---- Macronix bridge (report ID 0x01) ----

        static bool EnterMacronixAppMode(HidDevice dev)
        {
            byte id = ReportId(dev.Info);
            if (!dev.Write(new byte[] { id, 0x40 }, TimeoutMs)) return false;
            byte[]? mode = ReadUntil(dev, r => r.Length >= 4 && r[0] == id && r[1] == 0x40 ? r : null);
            return mode != null && mode[2] == 0x01 && mode[3] == 0x01;
        }

        static byte[]? SendMacronix(HidDevice dev, byte[] race, Func<byte[], bool> matches)
        {
            byte id = ReportId(dev.Info);
            var frame = new byte[3 + race.Length];
            frame[0] = id;
            frame[1] = 0x80;
            frame[2] = (byte)race.Length;
            Array.Copy(race, 0, frame, 3, race.Length);
            Trace?.Invoke(">", frame);
            if (!dev.Write(frame, TimeoutMs)) return null;

            return ReadUntil(dev, r =>
            {
                if (r.Length < 3 || r[0] != id || r[1] != 0x80 || r[2] > r.Length - 3) return null;
                return FindFrame(r, 3, r[2], matches);
            });
        }

        /// <summary>
        /// One input report can carry several RACE frames back to back: 50 49, frame type (01 answer, 08 notification),
        /// 5 more bytes, a 16-bit payload length, then the payload. The headset also sends notifications by itself
        /// (see <see cref="HeadsetEvents"/>), sometimes in bursts, so the answer we're waiting for is often not the
        /// first frame in the report - walk them all.
        /// </summary>
        static byte[]? FindFrame(byte[] report, int offset, int length, Func<byte[], bool> matches)
        {
            return Split(report, offset, length).FirstOrDefault(matches);
        }

        /// <summary>The RACE frames in one input report of the dongle, whatever they are.</summary>
        public static List<byte[]> Frames(byte[] r, byte reportId)
        {
            if (reportId == 0x02)
                return r.Length >= 2 && r[0] == 0x02 && r[1] <= r.Length - 2 ? Split(r, 2, r[1]) : new List<byte[]>();
            return r.Length >= 3 && r[0] == reportId && r[1] == 0x80 && r[2] <= r.Length - 3 ? Split(r, 3, r[2]) : new List<byte[]>();
        }

        static List<byte[]> Split(byte[] report, int offset, int length)
        {
            const int HeaderLength = 10; // 50 49 + 6 bytes, of which the last two are the payload length
            var frames = new List<byte[]>();
            int end = Math.Min(offset + length, report.Length);
            for (int pos = offset; pos + HeaderLength <= end;)
            {
                if (report[pos] != 0x50 || report[pos + 1] != 0x49) break;
                int payloadLength = report[pos + 8] | (report[pos + 9] << 8);
                int frameEnd = pos + HeaderLength + payloadLength;
                if (frameEnd > end) break;
                var frame = new byte[HeaderLength + payloadLength];
                Array.Copy(report, pos, frame, 0, frame.Length);
                frames.Add(frame);
                pos = frameEnd;
            }
            return frames;
        }

        /// <summary>Reads input reports until <paramref name="extract"/> returns non-null, or the timeout expires.
        /// The dongle interleaves unrelated reports, which are skipped.</summary>
        static byte[]? ReadUntil(HidDevice dev, Func<byte[], byte[]?> extract)
        {
            DateTime deadline = DateTime.UtcNow.AddMilliseconds(TimeoutMs);
            while (DateTime.UtcNow < deadline)
            {
                int remaining = (int)(deadline - DateTime.UtcNow).TotalMilliseconds;
                byte[]? r = dev.Read(Math.Max(remaining, 1));
                if (r == null) return null;
                Trace?.Invoke("<", r);
                byte[]? result = extract(r);
                if (result != null) return result;
            }
            return null;
        }

        /// <summary>Linear interpolation on the Li-ion discharge curve.</summary>
        public static int ToPercent(int mv)
        {
            if (mv <= Curve[0]) return 0;
            for (int i = 1; i < Curve.Length; i++)
            {
                if (mv < Curve[i])
                    return (i - 1) * 10 + (mv - Curve[i - 1]) * 10 / (Curve[i] - Curve[i - 1]);
            }
            return 100;
        }
    }
}
