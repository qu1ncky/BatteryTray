using System;
using System.Collections.Generic;
using System.Linq;
using System.Runtime.InteropServices;
using BatteryTray.Core;
using BatteryTray.Hid;
using Microsoft.Win32.SafeHandles;

namespace BatteryTray.Providers
{
    /// <summary>
    /// Catch-all for any USB device that declares the standard HID "Battery Strength" field (Generic Device Controls
    /// page 0x06, usage 0x20) in a feature report - common on generic 2.4 GHz receivers - which Windows never shows.
    /// Only feature reports are read (a GET_REPORT has no side effects), so devices with a dedicated provider and
    /// Bluetooth LE devices (Windows shows those itself, see BluetoothProvider) are skipped.
    /// </summary>
    public sealed class StandardHidBatteryProvider : IBatteryProvider
    {
        const ushort GenericDevicePage = 0x06, BatteryStrength = 0x20;
        const int HidpFeature = 2;
        const int HidpStatusSuccess = 0x00110000;

        /// <summary>Vendors read by their own providers.</summary>
        static readonly HashSet<ushort> OwnProviders = new() { 0x1532, 0x1038, 0x046D, 0x054C, 0x03F0, 0x0951, 0x1B1C };

        public string Name => "HID Battery Strength";

        public IReadOnlyList<DeviceReading> Poll()
        {
            var result = new List<DeviceReading>();
            foreach (HidInfo info in HidBus.Enumerate((v, _) => !OwnProviders.Contains(v)))
            {
                if (info.AccessDenied || info.FeatureLength == 0) continue;
                if (info.Path.Contains("{00001812-", StringComparison.OrdinalIgnoreCase)) continue; // BLE HID
                try
                {
                    int? level = ReadLevel(info);
                    if (level == null) continue;
                    result.Add(new DeviceReading
                    {
                        Id = $"hid:{info.VendorId:x4}:{info.ProductId:x4}", Name = info.Product ?? $"HID {info.VendorId:X4}:{info.ProductId:X4}",
                        Kind = KindOf(info), Via = "USB-приёмник", Online = true, Level = level,
                    });
                }
                catch (Exception)
                {
                    // A device that refuses the request just isn't listed
                }
            }
            // One entry per device even if several of its collections carry the field
            return result.GroupBy(r => r.Id).Select(g => g.First()).ToList();
        }

        public TimeSpan Delay(IReadOnlyList<DeviceReading> last)
        {
            return TimeSpan.FromMinutes(2);
        }

        static DeviceKind KindOf(HidInfo info)
        {
            if (info.UsagePage != 0x01) return DeviceKind.Other;
            return info.Usage switch
            {
                0x02 => DeviceKind.Mouse,
                0x06 => DeviceKind.Keyboard,
                0x04 or 0x05 => DeviceKind.Gamepad,
                _ => DeviceKind.Other,
            };
        }

        static int? ReadLevel(HidInfo info)
        {
            // No access rights needed for feature reports, even on collections Windows holds exclusively
            using SafeFileHandle h = CreateFile(info.Path, 0, 3, IntPtr.Zero, 3, 0, IntPtr.Zero);
            if (h.IsInvalid || !HidD_GetPreparsedData(h, out IntPtr pre)) return null;
            try
            {
                const int MaxCaps = 64; // HIDP_VALUE_CAPS is 72 bytes
                ushort count = MaxCaps;
                IntPtr caps = Marshal.AllocHGlobal(72 * MaxCaps);
                try
                {
                    if (HidP_GetSpecificValueCaps(HidpFeature, GenericDevicePage, 0, BatteryStrength, caps, ref count, pre)
                        != HidpStatusSuccess || count == 0)
                        return null;
                    // HIDP_VALUE_CAPS: ReportID at 2, LinkCollection at 6, LogicalMin at 40, LogicalMax at 44
                    byte reportId = Marshal.ReadByte(caps, 2);
                    ushort link = (ushort)Marshal.ReadInt16(caps, 6);
                    int min = Marshal.ReadInt32(caps, 40), max = Marshal.ReadInt32(caps, 44);

                    var report = new byte[info.FeatureLength];
                    report[0] = reportId;
                    if (!HidD_GetFeature(h, report, report.Length)) return null;
                    if (HidP_GetUsageValue(HidpFeature, GenericDevicePage, link, BatteryStrength, out uint raw, pre, report,
                            report.Length) != HidpStatusSuccess)
                        return null;
                    // Most devices declare 0-100 (or 0-255); a missing range is taken as a percentage
                    if (max <= min) (min, max) = (0, 100);
                    int percent = (int)Math.Round(((long)raw - min) * 100.0 / (max - min));
                    return percent is > 0 and <= 100 ? percent : null;
                }
                finally
                {
                    Marshal.FreeHGlobal(caps);
                }
            }
            finally
            {
                HidD_FreePreparsedData(pre);
            }
        }

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        static extern SafeFileHandle CreateFile(string name, uint access, uint share, IntPtr security, uint disposition,
            uint flags, IntPtr template);
        [DllImport("hid.dll")]
        static extern bool HidD_GetPreparsedData(SafeFileHandle h, out IntPtr data);
        [DllImport("hid.dll")]
        static extern bool HidD_FreePreparsedData(IntPtr data);
        [DllImport("hid.dll")]
        static extern bool HidD_GetFeature(SafeFileHandle h, byte[] buffer, int length);
        [DllImport("hid.dll")]
        static extern int HidP_GetSpecificValueCaps(int reportType, ushort usagePage, ushort linkCollection, ushort usage,
            IntPtr caps, ref ushort length, IntPtr data);
        [DllImport("hid.dll")]
        static extern int HidP_GetUsageValue(int reportType, ushort usagePage, ushort linkCollection, ushort usage,
            out uint value, IntPtr data, byte[] report, int reportLength);
    }
}
