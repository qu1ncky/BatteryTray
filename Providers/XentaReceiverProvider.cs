using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Linq;
using System.Threading;
using BatteryTray.Core;
using BatteryTray.Hid;

namespace BatteryTray.Providers
{
    /// <summary>
    /// Attack Shark mice on the Xenta 2.4 GHz receiver 1D57:FA60 ("2.4G Wireless Device"). Nothing is ever sent: the
    /// mouse pushes a 5-byte input report by itself about every 2 s on the receiver's collection with usage page
    /// 0x000A, and this only reads it, on its own GENERIC_READ handle:
    ///   03 MM 40 ST LL  -  report ID 3, model code, 0x40 = battery frame, state, level.
    /// What ST and LL mean depends on the model code, so only codes seen on real mice are read:
    ///   0x55 (X11), 0x4E (X3 Max), 0xE5 (V3 Pro, also the mouse of Attack-Shark-Battery-KDE):
    ///     ST 01 = on battery, LL = percent; 02 and 03 = on external power (cable or dock). 02 isn't "full": the X11
    ///     alternates 02 and 03 while topping up at 85% and the X3 Max sends 02 at 67%. The level they send on power is
    ///     frozen (85% for minutes on the cable, 97% right after unplugging), so none is shown then.
    ///   0x10 (X6 / R1): ST is the DPI stage and LL the level in tens; there is no charging state.
    /// A mouse with another code is listed without a level, and its frames go to the log so that it can be added.
    /// Each time the mouse links up it sends one "on battery, 100%" frame before the real one, so a reading only
    /// counts once two battery frames in a row agree. Other frames on the collection (03 MM 01 00 00 when the link
    /// blinks, configuration acknowledgements) carry no battery.
    /// On a USB cable the mouse leaves the receiver, which goes quiet, and shows up as its own USB device with the
    /// same collections (1D57:FA55 for the X11): it takes over the receiver's slot, as charging.
    /// Protocol facts from daeoxide/attackshark-battery-windows (docs/PROTOCOL.md), Attack-Shark-Battery-KDE
    /// (gitlab.com/gabriel17166) and the HaloBattery community fork (providers/inphic.py); own implementation.
    /// </summary>
    public sealed class XentaReceiverProvider : IBatteryProvider, IPushingProvider
    {
        const ushort Vid = 0x1D57, ReceiverPid = 0xFA60;
        /// <summary>The mouse on its USB cable: FA55 is the X11, 2155 the mouse of Attack-Shark-Battery-KDE.</summary>
        static readonly ushort[] WiredPids = { 0xFA55, 0x2155 };
        const ushort StatusPage = 0x000A;
        const byte ReportId = 0x03, BatteryFrame = 0x40, OnBattery = 0x01;
        const string ReceiverId = "xenta:1d57:fa60";
        const string DeviceName = "Attack Shark";
        /// <summary>How long a poll waits for a new listener's first reading, so the mouse doesn't show up as asleep
        /// for a moment: a reading takes two frames, 2 s apart.</summary>
        static readonly TimeSpan FirstReadingWait = TimeSpan.FromSeconds(5);

        /// <param name="SeenOn">The mouse the code was seen on, for the log. It isn't shown as the name: other
        /// models may well send the same code.</param>
        /// <param name="Tens">The 0x10 layout: the level in tens, no charging state.</param>
        readonly record struct Model(string SeenOn, bool Tens);

        static readonly Dictionary<byte, Model> Models = new()
        {
            [0x55] = new("Attack Shark X11", false),
            [0x4E] = new("Attack Shark X3 Max", false),
            [0xE5] = new("Attack Shark V3 Pro", false),
            [0x10] = new("Attack Shark X6 / R1", true),
        };

        /// <summary>What one battery frame says. Level is null on external power, or when the frame has none.</summary>
        internal readonly record struct Battery(bool OnPower, int? Level);

        /// <summary>What the frames of one status collection said so far.</summary>
        /// <param name="Running">Its listener still reads; false once the collection couldn't be opened or read.</param>
        /// <param name="Battery">The last reading two frames in a row agreed on, since the mouse linked up.</param>
        internal readonly record struct Status(bool Running, bool Online, byte? Code, Battery? Battery, string? Error);

        readonly StateStore state;
        readonly Dictionary<string, FrameListener> listeners = new(); // by collection path
        readonly Dictionary<string, string> loggedErrors = new();
        readonly HashSet<ushort> described = new();

        public XentaReceiverProvider(StateStore state)
        {
            this.state = state;
        }

        public string Name => "Xenta 2.4G";

        public event Action? Pushed;

        static bool Known(byte? code) => code is byte c && Models.ContainsKey(c);

        /// <summary>The model code and what the frame says, or null when the report isn't a battery frame. Battery
        /// is null when the model code is unknown or the frame doesn't fit its layout.</summary>
        internal static (byte Code, Battery? Battery)? Parse(byte[] report)
        {
            if (report.Length < 5 || report[0] != ReportId || report[2] != BatteryFrame) return null;
            byte code = report[1], st = report[3], level = report[4];
            if (!Models.TryGetValue(code, out Model model)) return (code, null);
            if (model.Tens)
                return level <= 10 ? (code, new Battery(false, level == 0 ? (int?)null : level * 10)) : (code, (Battery?)null);
            return st switch
            {
                // 0 is what devices of this kind send before they have measured: unknown, not empty
                OnBattery when level <= 100 => (code, new Battery(false, level == 0 ? (int?)null : level)),
                0x02 or 0x03 => (code, new Battery(true, null)),
                _ => (code, null),
            };
        }

        public IReadOnlyList<DeviceReading> Poll()
        {
            List<HidInfo> all = HidBus.Enumerate((v, p) => v == Vid && (p == ReceiverPid || Array.IndexOf(WiredPids, p) >= 0));
            List<HidInfo> status = all.Where(i => !i.AccessDenied && i.UsagePage == StatusPage && i.InputLength >= 5).ToList();
            Describe(all, status);
            Track(status);

            var receivers = status.Where(i => i.ProductId == ReceiverPid).Select(i => listeners[i.Path].Current()).ToList();
            var wired = status.Where(i => i.ProductId != ReceiverPid).Select(i => (i.ProductId, listeners[i.Path].Current())).ToList();
            // A third-party driver can lock the receiver's collections to administrators (see the README)
            bool locked = all.Any(i => i.ProductId == ReceiverPid && i.AccessDenied && i.InterfaceNumber == 2);
            return Build(receivers, wired, locked);
        }

        /// <summary>The readings for the receivers' and cables' status collections, in the order Windows lists them.</summary>
        internal List<DeviceReading> Build(IReadOnlyList<Status> receivers, IReadOnlyList<(ushort Pid, Status Status)> wired,
            bool receiverLocked)
        {
            var result = new List<DeviceReading>();
            foreach (Status s in receivers) result.Add(Reading(NextId(result, ReceiverId), s, cable: false));
            if (receivers.Count == 0 && receiverLocked)
            {
                result.Add(new DeviceReading
                {
                    Id = ReceiverId, Name = DeviceName, Kind = DeviceKind.Mouse, Via = Loc.T("via.24"),
                    Detail = Loc.T("d.needsAdmin"),
                });
            }

            // A mouse on the cable is the one that went quiet on its receiver when the cable went in
            var slots = Enumerable.Range(0, receivers.Count).ToList(); // the receivers' indexes in result
            foreach (var (pid, s) in wired)
            {
                if (!s.Online) continue; // a mouse on the cable that sends no battery frames: nothing to show
                int slot = slots.FindIndex(i => !result[i].Online);
                if (slot >= 0)
                {
                    int at = slots[slot];
                    slots.RemoveAt(slot);
                    result[at] = Reading(result[at].Id, s, cable: true);
                }
                else
                    result.Add(Reading(NextId(result, $"xenta:1d57:{pid:x4}"), s, cable: true));
            }
            return result;
        }

        public TimeSpan Delay(IReadOnlyList<DeviceReading> last)
        {
            // The listeners push every change; polling is for receivers and cables that come and go
            return last.Any(r => r.Online) ? TimeSpan.FromSeconds(30) : TimeSpan.FromSeconds(15);
        }

        /// <summary>Starts a listener for every new status collection and drops the ones that are gone.</summary>
        void Track(List<HidInfo> status)
        {
            foreach (string path in listeners.Keys.ToList())
            {
                FrameListener l = listeners[path];
                bool present = status.Any(i => i.Path == path);
                if (l.Running && present) continue;
                // A listener stops by itself when its device is unplugged; a collection still there that can't be
                // read is worth one line in the log (it gets a new listener right below and is tried again)
                if (present && l.Current().Error is string error
                    && (!loggedErrors.TryGetValue(path, out string? old) || old != error))
                {
                    loggedErrors[path] = error;
                    Log.Write($"Xenta 1d57:{l.ProductId:x4}: cannot read the status collection: {error}");
                }
                l.Dispose();
                listeners.Remove(path);
            }

            var fresh = new List<FrameListener>();
            foreach (HidInfo info in status)
            {
                if (listeners.ContainsKey(info.Path)) continue;
                var l = new FrameListener(info, () => Pushed?.Invoke());
                listeners[info.Path] = l;
                fresh.Add(l);
            }
            DateTime until = DateTime.UtcNow + FirstReadingWait;
            while (fresh.Any(l => !Settled(l.Current())) && DateTime.UtcNow < until) Thread.Sleep(100);
        }

        /// <summary>The listener has something to show: a confirmed reading, a model it can't read, or an error.</summary>
        static bool Settled(Status s) => !s.Running || s.Battery != null || (s.Online && !Known(s.Code));

        /// <summary>One line per USB ID and session with its HID collections: enough to see from a user's log
        /// whether a mouse that isn't read has the status collection at all.</summary>
        void Describe(List<HidInfo> all, List<HidInfo> status)
        {
            foreach (var device in all.GroupBy(i => i.ProductId))
            {
                if (!described.Add(device.Key)) continue;
                string collections = string.Join(", ", device.Select(i => i.AccessDenied
                    ? $"mi{i.InterfaceNumber} access denied"
                    : $"mi{i.InterfaceNumber} {i.UsagePage:X4}:{i.Usage:X4} in={i.InputLength} feat={i.FeatureLength}"));
                bool listening = status.Any(i => i.ProductId == device.Key);
                Log.Write($"Xenta 1d57:{device.Key:x4}: {(listening ? "listening on page 000A" : "no readable collection on page 000A")}; " +
                          $"collections: {collections}");
            }
        }

        static string NextId(List<DeviceReading> result, string id)
        {
            int same = result.Count(r => r.Id == id || r.Id.StartsWith(id + "#", StringComparison.Ordinal));
            return same == 0 ? id : id + "#" + (same + 1);
        }

        internal DeviceReading Reading(string id, Status s, bool cable)
        {
            var r = new DeviceReading
            {
                Id = id, Name = DeviceName, Kind = DeviceKind.Mouse, Via = Loc.T(cable ? "via.cable" : "via.24"),
            };
            // Silence is the mouse asleep, switched off or on Bluetooth - never an empty battery
            if (!s.Online) return r with { Detail = s.Error != null ? Loc.T("d.error", s.Error) : Loc.T("d.asleep") };
            r = r with { Online = true };
            if (!Known(s.Code)) return r with { Detail = Loc.T("d.unknownModel") };
            if (s.Battery is not Battery b) return r with { Detail = Loc.T("d.levelUnknown") };
            if (b.OnPower)
            {
                int? before = state.Get(id)?.Level;
                string why = Loc.T(cable ? "d.cableHidden" : "d.chargerHidden");
                return r with { Charging = true, Detail = why + (before is int was ? " · " + Loc.T("d.wasBefore", was) : "") };
            }
            if (b.Level == null) return r with { Detail = Loc.T("d.levelUnknown") };
            bool tens = Models[s.Code!.Value].Tens;
            return r with { Level = b.Level, Estimate = tens, Detail = tens ? Loc.T("d.tenSteps") : null };
        }

        /// <summary>
        /// What the frames of one collection said so far. No I/O and no clock of its own, so it can be fed by hand;
        /// not thread-safe (<see cref="FrameListener"/> locks around it). Frames it can't read go to the log, the
        /// first few of each, so that a new model can be added from a user's log.
        /// </summary>
        internal sealed class FrameTracker
        {
            /// <summary>The mouse sends every 2 s while it's linked; this much silence means it's gone.</summary>
            public static readonly TimeSpan SilentAfter = TimeSpan.FromSeconds(15);
            const int LoggedFramesMax = 16;

            readonly HashSet<string> loggedFrames = new();
            Battery? candidate;
            DateTime lastFrame;

            public bool Online { get; private set; }
            public byte? Code { get; private set; }
            /// <summary>The last reading two battery frames in a row agreed on, since the mouse linked up.</summary>
            public Battery? Confirmed { get; private set; }

            /// <summary>Takes one input report; true when what a poll would show has changed. Lines worth logging
            /// are added to <paramref name="log"/>.</summary>
            public bool Feed(byte[] report, DateTime now, List<string> log)
            {
                if (Parse(report) is not (byte code, var battery))
                {
                    // Unremarkable once the model is known (link blinks, acknowledgements)
                    if (!Known(Code)) LogFrame(log, report, "not a battery frame");
                    return false;
                }
                bool news = false;
                lastFrame = now;
                Online = true;
                if (Code != code)
                {
                    Code = code;
                    news = true;
                    log.Add(Models.TryGetValue(code, out Model m)
                        ? $"model code 0x{code:X2} (seen on the {m.SeenOn})"
                        : $"model code 0x{code:X2} is not known yet; its frames follow");
                }
                if (battery is Battery b)
                {
                    if (candidate == b && Confirmed != b)
                    {
                        Confirmed = b;
                        news = true;
                    }
                    candidate = b;
                }
                else
                    LogFrame(log, report, Known(code) ? "unknown state" : "unknown model");
                return news;
            }

            /// <summary>True when the mouse has just gone quiet.</summary>
            public bool Tick(DateTime now)
            {
                if (!Online || now - lastFrame < SilentAfter) return false;
                Lost();
                return true;
            }

            /// <summary>The link is gone. The next link-up starts with the false 100% again, so it's confirmed afresh.</summary>
            public void Lost()
            {
                Online = false;
                candidate = Confirmed = null;
            }

            void LogFrame(List<string> log, byte[] report, string why)
            {
                string hex = BitConverter.ToString(report).Replace('-', ' ').ToLowerInvariant();
                if (loggedFrames.Count < LoggedFramesMax && loggedFrames.Add(hex)) log.Add($"frame {hex} ({why})");
            }
        }

        /// <summary>Reads one status collection on its own thread into a <see cref="FrameTracker"/>.</summary>
        sealed class FrameListener : IDisposable
        {
            /// <summary>A listener that stops after running this long (receiver unplugged) asks for a poll at once.
            /// One that fails right away waits for the next poll, or polls would follow each other without a pause.</summary>
            static readonly TimeSpan PushOnStopAfter = TimeSpan.FromSeconds(10);

            readonly object gate = new();
            readonly FrameTracker tracker = new();
            readonly HidInfo info;
            readonly Action changed;
            readonly DateTime started = DateTime.UtcNow;
            volatile bool stopped, running = true;
            string? error;

            public FrameListener(HidInfo info, Action changed)
            {
                this.info = info;
                this.changed = changed;
                new Thread(Listen) { IsBackground = true, Name = "Xenta frames" }.Start();
            }

            public bool Running => running;
            public ushort ProductId => info.ProductId;

            public Status Current()
            {
                lock (gate) return new Status(running, tracker.Online, tracker.Code, tracker.Confirmed, error);
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
                    var log = new List<string>();
                    while (!stopped)
                    {
                        byte[]? report = dev.Read(1000);
                        bool news;
                        lock (gate)
                        {
                            news = report != null && tracker.Feed(report, DateTime.UtcNow, log);
                            news |= tracker.Tick(DateTime.UtcNow);
                        }
                        foreach (string line in log) Log.Write($"Xenta 1d57:{info.ProductId:x4}: {line}");
                        log.Clear();
                        if (news) changed();
                    }
                }
                catch (Exception e)
                {
                    lock (gate)
                    {
                        // Windows' own text ("Access is denied.") rather than "Cannot open <a long device path>"
                        error = e is Win32Exception w ? new Win32Exception(w.NativeErrorCode).Message : e.Message;
                        tracker.Lost();
                    }
                }
                finally
                {
                    running = false;
                    if (!stopped && DateTime.UtcNow - started > PushOnStopAfter) changed();
                }
            }
        }
    }
}
