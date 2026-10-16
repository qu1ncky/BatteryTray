using System.Collections.Generic;

namespace BatteryTray.Core
{
    /// <summary>
    /// Spots the moment a charge finishes: a device that was charging stops while full. Only for devices whose
    /// provider can see that while the cable is still in (<see cref="DeviceReading.ReportsFull"/>) - many devices
    /// report nothing new until the cable comes out, and then "charged" would only arrive after the user unplugged.
    /// One announcement per charge: chargers top a full battery up now and then, which must not repeat it.
    /// </summary>
    public sealed class ChargeDoneWatcher
    {
        const int FullPercent = 95;  // the headset's level is a voltage estimate in 5% steps
        const int RearmPercent = 90; // below this a new charge may be announced again

        readonly Dictionary<string, bool> wasCharging = new();
        readonly HashSet<string> announced = new();

        /// <summary>The rows whose charge finished since the last call.</summary>
        public List<DeviceRow> Finished(IEnumerable<DeviceRow> rows)
        {
            var done = new List<DeviceRow>();
            foreach (DeviceRow row in rows)
            {
                DeviceReading? r = row.Current;
                if (r == null) continue;
                if (!r.Online || !r.ReportsFull)
                {
                    // Turned off or out of range: whatever it does meanwhile wasn't seen
                    wasCharging.Remove(row.Id);
                    continue;
                }
                if (r.Level is int level && level <= RearmPercent) announced.Remove(row.Id);
                bool was = wasCharging.TryGetValue(row.Id, out bool w) && w;
                wasCharging[row.Id] = r.Charging;
                if (was && !r.Charging && r.Level >= FullPercent && announced.Add(row.Id)) done.Add(row);
            }
            return done;
        }
    }
}
