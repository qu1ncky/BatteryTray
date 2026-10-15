using System;
using System.Collections.Generic;
using System.Globalization;
using System.Text.RegularExpressions;
using BatteryTray.Core;
using BatteryTray.Hid;
using Windows.Devices.Bluetooth;

namespace BatteryTray.Providers
{
    /// <summary>
    /// Bluetooth devices whose battery level Windows itself knows (the level in Settings > Bluetooth &amp; devices).
    /// Windows keeps that property after a device disconnects, so the level only counts while the device is
    /// actually connected.
    /// </summary>
    public sealed class BluetoothProvider : IBatteryProvider
    {
        /// <summary>DEVPKEY_Bluetooth_Battery: one byte, 0-100, on the device node that carries the level
        /// (the LE device node, or the Hands-Free AG node of a classic headset).</summary>
        static readonly DevNodes.PropertyKey BatteryKey = new(new Guid("104EA319-6EE2-4701-BD47-8DDBF425BBE5"), 2);

        public string Name => "Bluetooth";

        public IReadOnlyList<DeviceReading> Poll()
        {
            var levels = new Dictionary<ulong, (bool LowEnergy, byte Level)>();
            foreach (var (instanceId, value) in DevNodes.ByteProperty(new[] { "BTHENUM", "BTHLE" }, BatteryKey))
            {
                ulong address = ParseAddress(instanceId);
                if (address == 0) continue;
                levels[address] = (instanceId.StartsWith("BTHLE", StringComparison.OrdinalIgnoreCase), value);
            }

            var result = new List<DeviceReading>();
            foreach (var (address, (lowEnergy, level)) in levels)
            {
                var (name, kind, connected) = Describe(address, lowEnergy);
                if (name == null) continue;
                result.Add(new DeviceReading
                {
                    Id = $"bt:{address:X12}", Name = name, Kind = kind, Via = "Bluetooth", Online = connected,
                    Level = connected && level <= 100 ? level : null,
                });
            }
            return result;
        }

        public TimeSpan Delay(IReadOnlyList<DeviceReading> last)
        {
            return TimeSpan.FromSeconds(60);
        }

        /// <summary>The device address inside an instance ID such as BTHLE\DEV_A1B2C3D4E5F6\7&amp;1234ABC&amp;0&amp;A1B2C3D4E5F6
        /// or BTHENUM\{0000111E-0000-1000-8000-00805F9B34FB}_LOCALMFG&amp;0002\7&amp;12345678&amp;0&amp;112233445566_C00000000.
        /// It is the last 12-digit group: a profile node's ID starts with the service GUID, which ends in 12 digits too.</summary>
        static ulong ParseAddress(string instanceId)
        {
            var m = Regex.Matches(instanceId, "(?<![0-9A-F])[0-9A-F]{12}(?![0-9A-F])", RegexOptions.IgnoreCase);
            return m.Count > 0 ? ulong.Parse(m[^1].Value, NumberStyles.HexNumber) : 0;
        }

        static (string? Name, DeviceKind Kind, bool Connected) Describe(ulong address, bool lowEnergy)
        {
            try
            {
                if (lowEnergy)
                {
                    using var le = BluetoothLEDevice.FromBluetoothAddressAsync(address).AsTask().GetAwaiter().GetResult();
                    if (le == null) return (null, DeviceKind.Other, false);
                    return (le.Name, KindOf(le.Name, le.Appearance), le.ConnectionStatus == BluetoothConnectionStatus.Connected);
                }
                using var classic = BluetoothDevice.FromBluetoothAddressAsync(address).AsTask().GetAwaiter().GetResult();
                if (classic == null) return (null, DeviceKind.Other, false);
                return (classic.Name, KindOf(classic.Name, classic.ClassOfDevice),
                    classic.ConnectionStatus == BluetoothConnectionStatus.Connected);
            }
            catch (Exception e)
            {
                Log.Write($"bluetooth {address:X12}: {e.Message}");
                return (null, DeviceKind.Other, false);
            }
        }

        static DeviceKind KindOf(string name, BluetoothLEAppearance appearance)
        {
            if (appearance != null && appearance.Category == BluetoothLEAppearanceCategories.HumanInterfaceDevice)
            {
                ushort sub = appearance.SubCategory;
                if (sub == BluetoothLEAppearanceSubcategories.Keyboard) return DeviceKind.Keyboard;
                if (sub == BluetoothLEAppearanceSubcategories.Mouse) return DeviceKind.Mouse;
                if (sub == BluetoothLEAppearanceSubcategories.Gamepad || sub == BluetoothLEAppearanceSubcategories.Joystick)
                    return DeviceKind.Gamepad;
            }
            return KindByName(name);
        }

        static DeviceKind KindOf(string name, BluetoothClassOfDevice cod)
        {
            if (cod != null)
            {
                if (cod.MajorClass == BluetoothMajorClass.AudioVideo) return DeviceKind.Headset;
                if (cod.MajorClass == BluetoothMajorClass.Peripheral)
                {
                    // Bits 6-7 of the minor class: 01 = keyboard, 10 = pointing device
                    uint kind = (cod.RawValue >> 6) & 0x3;
                    if (kind == 1) return DeviceKind.Keyboard;
                    if (kind == 2) return DeviceKind.Mouse;
                    if (cod.MinorClass is BluetoothMinorClass.PeripheralGamepad or BluetoothMinorClass.PeripheralJoystick)
                        return DeviceKind.Gamepad;
                }
            }
            return KindByName(name);
        }

        static DeviceKind KindByName(string name)
        {
            string n = name.ToLowerInvariant();
            if (Regex.IsMatch(n, "mouse|мыш")) return DeviceKind.Mouse;
            if (Regex.IsMatch(n, "controller|gamepad|xbox|dualsense|dualshock|joy")) return DeviceKind.Gamepad;
            if (Regex.IsMatch(n, "keyboard|клав|rainy|keychron")) return DeviceKind.Keyboard;
            if (Regex.IsMatch(n, "headset|headphone|buds|barracuda|kraken|arctis|airpods|наушн")) return DeviceKind.Headset;
            return DeviceKind.Other;
        }
    }
}
