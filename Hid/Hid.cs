// HID enumeration and I/O with timeouts.
// Adapted from github.com/mehdibouchami/razer-barracuda-x-battery (MIT License, Copyright (c) 2026 Mehdi Bouchami).
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text.RegularExpressions;
using System.Threading;
using Microsoft.Win32.SafeHandles;

namespace BatteryTray.Hid
{
    public sealed class HidInfo
    {
        public string Path = "";
        public ushort VendorId;
        public ushort ProductId;
        public ushort UsagePage;
        public ushort Usage;
        public int InputLength;
        public int OutputLength;
        public int FeatureLength;
        public byte OutputReportId;
        public string? Product;
        /// <summary>The collection exists but its security descriptor won't let this process open it.</summary>
        public bool AccessDenied;

        /// <summary>USB interface number from the device path ("...&amp;mi_02..."), or -1.</summary>
        public int InterfaceNumber
        {
            get
            {
                var m = Regex.Match(Path, "&mi_([0-9a-f]{2})", RegexOptions.IgnoreCase);
                return m.Success ? Convert.ToInt32(m.Groups[1].Value, 16) : -1;
            }
        }

        public override string ToString()
        {
            return $"{VendorId:X4}:{ProductId:X4} page=0x{UsagePage:X4} usage=0x{Usage:X4} in={InputLength} out={OutputLength} " +
                   $"feat={FeatureLength} outId=0x{OutputReportId:X2}{(AccessDenied ? " ACCESS DENIED" : "")}";
        }
    }

    public static class HidBus
    {
        const int ErrorAccessDenied = 5;

        public static List<HidInfo> Enumerate(Func<ushort, ushort, bool> match)
        {
            var result = new List<HidInfo>();
            Native.HidD_GetHidGuid(out Guid hidGuid);
            IntPtr set = Native.SetupDiGetClassDevs(ref hidGuid, IntPtr.Zero, IntPtr.Zero,
                Native.DIGCF_PRESENT | Native.DIGCF_DEVICEINTERFACE);
            if (set == new IntPtr(-1)) return result;
            try
            {
                var ifData = new Native.SP_DEVICE_INTERFACE_DATA();
                ifData.cbSize = Marshal.SizeOf(ifData);
                for (uint i = 0; Native.SetupDiEnumDeviceInterfaces(set, IntPtr.Zero, ref hidGuid, i, ref ifData); i++)
                {
                    string? path = GetPath(set, ref ifData);
                    if (path == null || !TryParseIds(path, out ushort vid, out ushort pid) || !match(vid, pid)) continue;
                    HidInfo? info = Query(path, vid, pid);
                    if (info != null) result.Add(info);
                }
            }
            finally
            {
                Native.SetupDiDestroyDeviceInfoList(set);
            }
            return result;
        }

        static bool TryParseIds(string path, out ushort vid, out ushort pid)
        {
            var m = Regex.Match(path, "vid_([0-9a-f]{4})&pid_([0-9a-f]{4})", RegexOptions.IgnoreCase);
            vid = m.Success ? Convert.ToUInt16(m.Groups[1].Value, 16) : (ushort)0;
            pid = m.Success ? Convert.ToUInt16(m.Groups[2].Value, 16) : (ushort)0;
            return m.Success;
        }

        static string? GetPath(IntPtr set, ref Native.SP_DEVICE_INTERFACE_DATA ifData)
        {
            Native.SetupDiGetDeviceInterfaceDetail(set, ref ifData, IntPtr.Zero, 0, out int size, IntPtr.Zero);
            if (size <= 0) return null;
            IntPtr buf = Marshal.AllocHGlobal(size);
            try
            {
                // cbSize of SP_DEVICE_INTERFACE_DETAIL_DATA_W: 8 on x64
                Marshal.WriteInt32(buf, IntPtr.Size == 8 ? 8 : 6);
                if (!Native.SetupDiGetDeviceInterfaceDetail(set, ref ifData, buf, size, out size, IntPtr.Zero))
                    return null;
                return Marshal.PtrToStringUni(buf + 4);
            }
            finally
            {
                Marshal.FreeHGlobal(buf);
            }
        }

        static HidInfo? Query(string path, ushort vid, ushort pid)
        {
            // Zero access rights: enough to read attributes even for devices opened exclusively
            using SafeFileHandle h = Native.CreateFile(path, 0, Native.FILE_SHARE_READ | Native.FILE_SHARE_WRITE,
                IntPtr.Zero, Native.OPEN_EXISTING, 0, IntPtr.Zero);
            if (h.IsInvalid)
            {
                return Marshal.GetLastWin32Error() == ErrorAccessDenied
                    ? new HidInfo { Path = path, VendorId = vid, ProductId = pid, AccessDenied = true }
                    : null;
            }
            var info = new HidInfo { Path = path, VendorId = vid, ProductId = pid };
            if (Native.HidD_GetPreparsedData(h, out IntPtr pre))
            {
                var caps = new Native.HIDP_CAPS();
                if (Native.HidP_GetCaps(pre, ref caps) == Native.HIDP_STATUS_SUCCESS)
                {
                    info.UsagePage = caps.UsagePage;
                    info.Usage = caps.Usage;
                    info.InputLength = caps.InputReportByteLength;
                    info.OutputLength = caps.OutputReportByteLength;
                    info.FeatureLength = caps.FeatureReportByteLength;
                    info.OutputReportId = GetOutputReportId(pre, caps);
                }
                Native.HidD_FreePreparsedData(pre);
            }
            var name = new byte[256];
            if (Native.HidD_GetProductString(h, name, name.Length))
            {
                string s = System.Text.Encoding.Unicode.GetString(name).TrimEnd('\0');
                if (s.Length > 0) info.Product = s;
            }
            return info;
        }

        static byte GetOutputReportId(IntPtr preparsed, Native.HIDP_CAPS caps)
        {
            // Output reports are described by value caps (a byte array) or, less often, button caps
            byte id = ReadCapsReportId(Native.HidP_GetValueCaps, caps.NumberOutputValueCaps, preparsed);
            return id != 0 ? id : ReadCapsReportId(Native.HidP_GetButtonCaps, caps.NumberOutputButtonCaps, preparsed);
        }

        static byte ReadCapsReportId(Native.GetCapsFn get, ushort count, IntPtr preparsed)
        {
            // HIDP_VALUE_CAPS and HIDP_BUTTON_CAPS are both 72 bytes; ReportID is the byte at offset 2
            const int CapsSize = 72;
            if (count == 0) return 0;
            IntPtr buf = Marshal.AllocHGlobal(CapsSize * count);
            try
            {
                if (get(Native.HidP_Output, buf, ref count, preparsed) != Native.HIDP_STATUS_SUCCESS || count == 0)
                    return 0;
                return Marshal.ReadByte(buf, 2);
            }
            finally
            {
                Marshal.FreeHGlobal(buf);
            }
        }
    }

    /// <summary>Read/write handle to one HID collection.</summary>
    public sealed class HidDevice : IDisposable
    {
        readonly SafeFileHandle handle;
        public readonly HidInfo Info;

        /// <param name="overlapped">Overlapped I/O is needed for <see cref="Read"/>/<see cref="Write"/> with a
        /// timeout; feature-report-only devices are opened synchronously.</param>
        public HidDevice(HidInfo info, bool overlapped = true)
        {
            Info = info;
            handle = Native.CreateFile(info.Path, Native.GENERIC_READ | Native.GENERIC_WRITE,
                Native.FILE_SHARE_READ | Native.FILE_SHARE_WRITE, IntPtr.Zero, Native.OPEN_EXISTING,
                overlapped ? Native.FILE_FLAG_OVERLAPPED : 0, IntPtr.Zero);
            if (handle.IsInvalid) throw new Win32Exception(Marshal.GetLastWin32Error(), "Cannot open " + info.Path);
        }

        public void Flush()
        {
            Native.HidD_FlushQueue(handle);
        }

        /// <summary>Synchronously fetches an input report over the control pipe (HidD_GetInputReport).</summary>
        public byte[]? GetInputReport(byte reportId)
        {
            var buf = new byte[Info.InputLength];
            buf[0] = reportId;
            return Native.HidD_GetInputReport(handle, buf, buf.Length) ? buf : null;
        }

        public bool SetFeature(byte[] report)
        {
            return Native.HidD_SetFeature(handle, report, report.Length);
        }

        public byte[]? GetFeature(byte reportId)
        {
            var buf = new byte[Info.FeatureLength];
            buf[0] = reportId;
            return Native.HidD_GetFeature(handle, buf, buf.Length) ? buf : null;
        }

        public bool Write(byte[] report, int timeoutMs)
        {
            var buf = new byte[Info.OutputLength];
            Array.Copy(report, buf, Math.Min(report.Length, buf.Length));
            return Io(buf, true, timeoutMs) == buf.Length;
        }

        /// <summary>Returns the input report, or null on timeout.</summary>
        public byte[]? Read(int timeoutMs)
        {
            var buf = new byte[Info.InputLength];
            int n = Io(buf, false, timeoutMs);
            if (n <= 0) return null;
            if (n < buf.Length) Array.Resize(ref buf, n);
            return buf;
        }

        int Io(byte[] data, bool write, int timeoutMs)
        {
            IntPtr buf = Marshal.AllocHGlobal(data.Length);
            IntPtr ov = Marshal.AllocHGlobal(Marshal.SizeOf<NativeOverlapped>());
            using var evt = new ManualResetEvent(false);
            try
            {
                var o = new NativeOverlapped { EventHandle = evt.SafeWaitHandle.DangerousGetHandle() };
                Marshal.StructureToPtr(o, ov, false);
                if (write) Marshal.Copy(data, 0, buf, data.Length);

                bool ok = write
                    ? Native.WriteFile(handle, buf, data.Length, out _, ov)
                    : Native.ReadFile(handle, buf, data.Length, out _, ov);
                if (!ok)
                {
                    int err = Marshal.GetLastWin32Error();
                    if (err != Native.ERROR_IO_PENDING) throw new Win32Exception(err);
                    if (!evt.WaitOne(timeoutMs)) Native.CancelIoEx(handle, ov);
                }
                // bWait=true: after a cancel this returns once the cancellation has completed,
                // so the buffers are safe to free afterwards.
                if (!Native.GetOverlappedResult(handle, ov, out int transferred, true))
                {
                    int err = Marshal.GetLastWin32Error();
                    if (err == Native.ERROR_OPERATION_ABORTED) return 0;
                    throw new Win32Exception(err);
                }
                if (!write) Marshal.Copy(buf, data, 0, transferred);
                return transferred;
            }
            finally
            {
                Marshal.FreeHGlobal(ov);
                Marshal.FreeHGlobal(buf);
            }
        }

        public void Dispose()
        {
            handle.Dispose();
        }
    }

    static class Native
    {
        public const int DIGCF_PRESENT = 0x2, DIGCF_DEVICEINTERFACE = 0x10;
        public const uint GENERIC_READ = 0x80000000, GENERIC_WRITE = 0x40000000;
        public const uint FILE_SHARE_READ = 1, FILE_SHARE_WRITE = 2, OPEN_EXISTING = 3;
        public const uint FILE_FLAG_OVERLAPPED = 0x40000000;
        public const int ERROR_IO_PENDING = 997, ERROR_OPERATION_ABORTED = 995;
        public const int HIDP_STATUS_SUCCESS = 0x00110000;
        public const int HidP_Output = 1;

        [StructLayout(LayoutKind.Sequential)]
        public struct SP_DEVICE_INTERFACE_DATA
        {
            public int cbSize;
            public Guid InterfaceClassGuid;
            public int Flags;
            public IntPtr Reserved;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct HIDP_CAPS
        {
            public ushort Usage;
            public ushort UsagePage;
            public ushort InputReportByteLength;
            public ushort OutputReportByteLength;
            public ushort FeatureReportByteLength;
            [MarshalAs(UnmanagedType.ByValArray, SizeConst = 17)]
            public ushort[] Reserved;
            public ushort NumberLinkCollectionNodes;
            public ushort NumberInputButtonCaps;
            public ushort NumberInputValueCaps;
            public ushort NumberInputDataIndices;
            public ushort NumberOutputButtonCaps;
            public ushort NumberOutputValueCaps;
            public ushort NumberOutputDataIndices;
            public ushort NumberFeatureButtonCaps;
            public ushort NumberFeatureValueCaps;
            public ushort NumberFeatureDataIndices;
        }

        public delegate int GetCapsFn(int reportType, IntPtr caps, ref ushort length, IntPtr data);

        [DllImport("hid.dll")]
        public static extern void HidD_GetHidGuid(out Guid guid);
        [DllImport("hid.dll", SetLastError = true)]
        public static extern bool HidD_GetPreparsedData(SafeFileHandle h, out IntPtr data);
        [DllImport("hid.dll")]
        public static extern bool HidD_FreePreparsedData(IntPtr data);
        [DllImport("hid.dll")]
        public static extern int HidP_GetCaps(IntPtr data, ref HIDP_CAPS caps);
        [DllImport("hid.dll")]
        public static extern int HidP_GetValueCaps(int reportType, IntPtr caps, ref ushort length, IntPtr data);
        [DllImport("hid.dll")]
        public static extern int HidP_GetButtonCaps(int reportType, IntPtr caps, ref ushort length, IntPtr data);
        [DllImport("hid.dll")]
        public static extern bool HidD_FlushQueue(SafeFileHandle h);
        [DllImport("hid.dll", SetLastError = true)]
        public static extern bool HidD_GetInputReport(SafeFileHandle h, byte[] buffer, int length);
        [DllImport("hid.dll", SetLastError = true)]
        public static extern bool HidD_SetFeature(SafeFileHandle h, byte[] buffer, int length);
        [DllImport("hid.dll", SetLastError = true)]
        public static extern bool HidD_GetFeature(SafeFileHandle h, byte[] buffer, int length);
        [DllImport("hid.dll", SetLastError = true)]
        public static extern bool HidD_GetProductString(SafeFileHandle h, byte[] buffer, int length);

        [DllImport("setupapi.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        public static extern IntPtr SetupDiGetClassDevs(ref Guid classGuid, IntPtr enumerator, IntPtr parent, int flags);
        [DllImport("setupapi.dll", SetLastError = true)]
        public static extern bool SetupDiEnumDeviceInterfaces(IntPtr set, IntPtr devInfo, ref Guid guid, uint index,
            ref SP_DEVICE_INTERFACE_DATA data);
        [DllImport("setupapi.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        public static extern bool SetupDiGetDeviceInterfaceDetail(IntPtr set, ref SP_DEVICE_INTERFACE_DATA data,
            IntPtr detail, int detailSize, out int requiredSize, IntPtr devInfo);
        [DllImport("setupapi.dll")]
        public static extern bool SetupDiDestroyDeviceInfoList(IntPtr set);

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        public static extern SafeFileHandle CreateFile(string name, uint access, uint share, IntPtr security,
            uint disposition, uint flags, IntPtr template);
        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern bool ReadFile(SafeFileHandle h, IntPtr buf, int len, out int read, IntPtr ov);
        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern bool WriteFile(SafeFileHandle h, IntPtr buf, int len, out int written, IntPtr ov);
        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern bool GetOverlappedResult(SafeFileHandle h, IntPtr ov, out int transferred, bool wait);
        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern bool CancelIoEx(SafeFileHandle h, IntPtr ov);
    }
}
