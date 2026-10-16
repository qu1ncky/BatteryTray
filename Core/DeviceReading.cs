using System;

namespace BatteryTray.Core
{
    public enum DeviceKind { Headset, Mouse, Keyboard, Gamepad, Other }

    /// <summary>One device's state as a provider saw it at one moment.</summary>
    public sealed record DeviceReading
    {
        /// <summary>Stable key for settings and history, e.g. "razer:barracuda-x" or "bt:A1B2C3D4E5F6".</summary>
        public required string Id { get; init; }
        public required string Name { get; init; }
        public DeviceKind Kind { get; init; } = DeviceKind.Other;
        /// <summary>How the device is reached, as shown: "2.4 GHz", "Bluetooth", "cable"...</summary>
        public required string Via { get; init; }
        public bool Online { get; init; }
        /// <summary>0-100, or null when the device doesn't report a level right now. Never a stale level passed off
        /// as current: a device that can't be read while charging reports null and says why in <see cref="Detail"/>.</summary>
        public int? Level { get; init; }
        /// <summary><see cref="Level"/> is our estimate (e.g. from the voltage), not the device's own number.</summary>
        public bool Estimate { get; init; }
        public bool Charging { get; init; }
        /// <summary>The charger is still connected and the device itself reports the battery full.</summary>
        public bool FullOnCharger { get; init; }
        /// <summary>Extra text for the UI: the voltage, or why there is no reading.</summary>
        public string? Detail { get; init; }
        public DateTime Time { get; init; } = DateTime.Now;
    }

    /// <summary>A device as the UI shows it: what the provider reports now, plus the last level we ever saw.</summary>
    public sealed record DeviceRow(string Id, DeviceReading? Current, DeviceReading? Last, string Name)
    {
        public DeviceReading Any => (Current ?? Last)!;
        public DeviceKind Kind => Any.Kind;
        public bool Online => Current?.Online == true;
    }
}
