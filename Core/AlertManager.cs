using System;
using System.Collections.Generic;

namespace BatteryTray.Core
{
    public sealed record AlertItem(string Id, string Name, DeviceKind Kind, int Level, bool Critical);

    /// <summary>
    /// Decides which devices need a low-battery reminder right now. A reminder stays due until the user snoozes it
    /// (it comes back after the repeat interval) or mutes it (quiet until the device is charged). Reaching the
    /// critical level breaks through a snooze or mute once. Charging, or a level clearly above the threshold,
    /// re-arms everything.
    /// </summary>
    public sealed class AlertManager
    {
        const int RearmMargin = 5;

        sealed class State
        {
            public DateTime SnoozedUntil;
            public bool Muted;
            public bool CriticalSeen;
        }

        readonly Dictionary<string, State> states = new();

        public IReadOnlyList<AlertItem> Due(IEnumerable<DeviceRow> rows, AppSettings settings, DateTime now)
        {
            var due = new List<AlertItem>();
            foreach (var row in rows)
            {
                DeviceReading? r = row.Current;
                if (r == null) continue; // provider lost the device (dongle unplugged): keep its state as is
                DeviceSettings ds = settings.For(row.Id);
                if (ds.Hidden || !ds.Alerts || r.Charging || r.Level > settings.LowPercent + RearmMargin)
                {
                    states.Remove(row.Id);
                    continue;
                }
                if (!r.Online || r.Level is not int level || level > settings.LowPercent) continue;

                if (!states.TryGetValue(row.Id, out var st)) states[row.Id] = st = new State();
                bool critical = level <= settings.CriticalPercent;
                if (critical && !st.CriticalSeen)
                {
                    st.CriticalSeen = true;
                    st.Muted = false;
                    st.SnoozedUntil = default;
                }
                if (st.Muted || now < st.SnoozedUntil) continue;
                due.Add(new AlertItem(row.Id, row.Name, row.Kind, level, critical));
            }
            return due;
        }

        public void Snooze(IEnumerable<string> ids, TimeSpan duration)
        {
            foreach (string id in ids) Get(id).SnoozedUntil = DateTime.Now + duration;
        }

        public void MuteUntilCharged(IEnumerable<string> ids)
        {
            foreach (string id in ids) Get(id).Muted = true;
        }

        State Get(string id)
        {
            if (!states.TryGetValue(id, out var st)) states[id] = st = new State();
            return st;
        }
    }
}
