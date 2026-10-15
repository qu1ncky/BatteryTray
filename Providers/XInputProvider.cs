using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using BatteryTray.Core;

namespace BatteryTray.Providers
{
    /// <summary>
    /// Xbox controllers through the Xbox Wireless Adapter (or any XInput gamepad on batteries). XInput only knows
    /// four levels - empty, low, medium, full - so the percentage is a rough stand-in. Controllers on Bluetooth are
    /// skipped when XInput reveals their product ID: Windows reports their exact level, see BluetoothProvider.
    /// </summary>
    public sealed class XInputProvider : IBatteryProvider
    {
        const int ErrorSuccess = 0;
        const byte DevTypeGamepad = 0;
        const byte TypeDisconnected = 0, TypeWired = 1;

        /// <summary>Product IDs Xbox controllers use over Bluetooth (classic and LE), as listed by SDL.</summary>
        static readonly HashSet<ushort> BluetoothPids = new() { 0x02E0, 0x02FD, 0x0B05, 0x0B0C, 0x0B13, 0x0B20, 0x0B21, 0x0B22 };

        public string Name => "XInput";

        public IReadOnlyList<DeviceReading> Poll()
        {
            var result = new List<DeviceReading>();
            for (int slot = 0; slot < 4; slot++)
            {
                if (XInputGetBatteryInformation(slot, DevTypeGamepad, out Battery b) != ErrorSuccess) continue;
                if (b.Type is TypeDisconnected or TypeWired) continue;
                if (XInputGetCapabilitiesEx(1, slot, 0, out CapabilitiesEx caps) == ErrorSuccess
                    && caps.VendorId == 0x045E && BluetoothPids.Contains(caps.ProductId))
                    continue;

                var (level, text) = b.Level switch
                {
                    0 => (5, "почти пусто"),
                    1 => (15, "низкий"),
                    2 => (50, "средний"),
                    _ => (100, "полный"),
                };
                result.Add(new DeviceReading
                {
                    Id = $"xinput:{slot}", Name = slot == 0 ? "Геймпад Xbox" : $"Геймпад Xbox #{slot + 1}",
                    Kind = DeviceKind.Gamepad, Via = "XInput", Online = true, Level = level,
                    Detail = $"уровень «{text}» (XInput знает только 4 уровня)",
                });
            }
            return result;
        }

        public TimeSpan Delay(IReadOnlyList<DeviceReading> last)
        {
            return TimeSpan.FromSeconds(60);
        }

        [StructLayout(LayoutKind.Sequential)]
        struct Battery
        {
            public byte Type;
            public byte Level;
        }

        /// <summary>XINPUT_CAPABILITIES_EX, returned by the undocumented ordinal 108 of xinput1_4.dll.</summary>
        [StructLayout(LayoutKind.Sequential)]
        struct CapabilitiesEx
        {
            public byte Type, SubType;
            public ushort Flags;
            // XINPUT_GAMEPAD (12 bytes)
            public ushort Buttons;
            public byte LeftTrigger, RightTrigger;
            public short ThumbLX, ThumbLY, ThumbRX, ThumbRY;
            // XINPUT_VIBRATION
            public ushort LeftMotor, RightMotor;
            public ushort VendorId, ProductId, ProductVersion, Reserved1;
            public uint Reserved2;
        }

        [DllImport("xinput1_4.dll")]
        static extern int XInputGetBatteryInformation(int userIndex, byte devType, out Battery battery);

        [DllImport("xinput1_4.dll", EntryPoint = "#108")]
        static extern int XInputGetCapabilitiesEx(int reserved, int userIndex, int flags, out CapabilitiesEx caps);
    }
}
