using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

namespace BatteryTray.Hid
{
    /// <summary>Reads device-node properties through SetupAPI (what Device Manager's "Details" tab shows).</summary>
    public static class DevNodes
    {
        public readonly record struct PropertyKey(Guid FormatId, int Id);

        const int DIGCF_PRESENT = 0x2, DIGCF_ALLCLASSES = 0x4;
        const int DEVPROP_TYPE_BYTE = 0x3;

        /// <summary>Every present device node under the given enumerators ("BTHENUM", "BTHLE"...) that has a
        /// one-byte value for <paramref name="key"/>.</summary>
        public static List<(string InstanceId, byte Value)> ByteProperty(IEnumerable<string> enumerators, PropertyKey key)
        {
            var result = new List<(string, byte)>();
            foreach (string enumerator in enumerators)
            {
                IntPtr set = SetupDiGetClassDevsW(IntPtr.Zero, enumerator, IntPtr.Zero, DIGCF_PRESENT | DIGCF_ALLCLASSES);
                if (set == new IntPtr(-1)) continue;
                try
                {
                    var data = new SP_DEVINFO_DATA { cbSize = Marshal.SizeOf<SP_DEVINFO_DATA>() };
                    var devKey = new DEVPROPKEY { fmtid = key.FormatId, pid = key.Id };
                    var value = new byte[4];
                    var id = new StringBuilder(512);
                    for (int i = 0; SetupDiEnumDeviceInfo(set, i, ref data); i++)
                    {
                        if (!SetupDiGetDevicePropertyW(set, ref data, ref devKey, out int type, value, value.Length, out _, 0)
                            || type != DEVPROP_TYPE_BYTE)
                            continue;
                        if (!SetupDiGetDeviceInstanceIdW(set, ref data, id, id.Capacity, out _)) continue;
                        result.Add((id.ToString(), value[0]));
                    }
                }
                finally
                {
                    SetupDiDestroyDeviceInfoList(set);
                }
            }
            return result;
        }

        [StructLayout(LayoutKind.Sequential)]
        struct SP_DEVINFO_DATA
        {
            public int cbSize;
            public Guid ClassGuid;
            public int DevInst;
            public IntPtr Reserved;
        }

        [StructLayout(LayoutKind.Sequential)]
        struct DEVPROPKEY
        {
            public Guid fmtid;
            public int pid;
        }

        [DllImport("setupapi.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        static extern IntPtr SetupDiGetClassDevsW(IntPtr classGuid, string enumerator, IntPtr parent, int flags);
        [DllImport("setupapi.dll", SetLastError = true)]
        static extern bool SetupDiEnumDeviceInfo(IntPtr set, int index, ref SP_DEVINFO_DATA data);
        [DllImport("setupapi.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        static extern bool SetupDiGetDeviceInstanceIdW(IntPtr set, ref SP_DEVINFO_DATA data, StringBuilder id, int size,
            out int required);
        [DllImport("setupapi.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        static extern bool SetupDiGetDevicePropertyW(IntPtr set, ref SP_DEVINFO_DATA data, ref DEVPROPKEY key,
            out int type, byte[] buffer, int size, out int required, int flags);
        [DllImport("setupapi.dll")]
        static extern bool SetupDiDestroyDeviceInfoList(IntPtr set);
    }
}
