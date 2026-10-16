using System;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Security;
using System.Security.Principal;
using System.Text;

namespace BatteryTray.Core
{
    public static class Elevation
    {
        public static readonly bool IsElevated =
            new WindowsPrincipal(WindowsIdentity.GetCurrent()).IsInRole(WindowsBuiltInRole.Administrator);
    }

    /// <summary>
    /// Starts the app at logon through a Task Scheduler task. When installed in Program Files the task runs it
    /// with highest privileges: on some PCs a third-party driver makes a dongle's vendor HID collection admin-only
    /// (e.g. the G-SPY driver vkbms.inf claims every FF00:0001 collection, the Razer one included, and restricts it
    /// to SYSTEM and Administrators), and a logon task gets admin rights without a UAC prompt. Anywhere else the
    /// task runs with normal rights: an elevated task must never start an exe that any program could replace.
    /// </summary>
    public static class AutoStart
    {
        public const string TaskName = "BatteryTray";
        /// <summary>Passed by the task: start in the tray without opening the window.</summary>
        public const string BackgroundSwitch = "--background";

        static string Exe => Environment.ProcessPath ?? throw new InvalidOperationException("no process path");

        /// <summary>True when this exe sits in Program Files, which only administrators can write to.</summary>
        public static bool IsInstalled
        {
            get
            {
                string programFiles = Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles);
                return Exe.StartsWith(programFiles + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase);
            }
        }

        public static bool IsRegistered()
        {
            return Run("schtasks.exe", $"/Query /TN \"{TaskName}\"") == 0;
        }

        /// <summary>Starts the task now. The user owns it, so this needs no elevation and shows no UAC prompt,
        /// yet the app it starts runs with admin rights.</summary>
        public static bool RunTask()
        {
            return Run("schtasks.exe", $"/Run /TN \"{TaskName}\"") == 0;
        }

        /// <summary>Registers or removes the task. An elevated task needs admin rights to create, so outside an
        /// elevated process that asks for them (UAC); a normal-rights task for a portable copy doesn't.</summary>
        public static bool Set(bool enabled)
        {
            if (Elevation.IsElevated) return enabled ? Register() : Unregister();
            if (!IsInstalled && (enabled ? Register() : Unregister())) return true;
            return RunSelfElevated(enabled ? "--register-task" : "--unregister-task");
        }

        public static bool Register()
        {
            string exe = Exe;
            string user = WindowsIdentity.GetCurrent().Name;
            string runLevel = IsInstalled ? "HighestAvailable" : "LeastPrivilege";
            string xml = $@"<?xml version=""1.0"" encoding=""UTF-16""?>
<Task version=""1.4"" xmlns=""http://schemas.microsoft.com/windows/2004/02/mit/task"">
  <RegistrationInfo>
    <Description>BatteryTray: battery levels of wireless devices in the tray, with low battery reminders</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{SecurityElement.Escape(user)}</UserId>
      <Delay>PT5S</Delay>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id=""Author"">
      <UserId>{SecurityElement.Escape(user)}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>{runLevel}</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>6</Priority>
    <RestartOnFailure>
      <Interval>PT1M</Interval>
      <Count>3</Count>
    </RestartOnFailure>
  </Settings>
  <Actions Context=""Author"">
    <Exec>
      <Command>{SecurityElement.Escape(exe)}</Command>
      <Arguments>{BackgroundSwitch}</Arguments>
      <WorkingDirectory>{SecurityElement.Escape(Path.GetDirectoryName(exe))}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>";
            string file = Path.Combine(Path.GetTempPath(), "BatteryTray-task.xml");
            try
            {
                File.WriteAllText(file, xml, Encoding.Unicode);
                int code = Run("schtasks.exe", $"/Create /TN \"{TaskName}\" /XML \"{file}\" /F");
                Log.Write($"register logon task: exit {code}");
                return code == 0;
            }
            finally
            {
                File.Delete(file);
            }
        }

        public static bool Unregister()
        {
            int code = Run("schtasks.exe", $"/Delete /TN \"{TaskName}\" /F");
            Log.Write($"unregister logon task: exit {code}");
            return code == 0;
        }

        /// <summary>Runs this exe with admin rights (UAC prompt) and the given switch; true when it exits with 0.</summary>
        public static bool RunSelfElevated(string argument)
        {
            try
            {
                var psi = new ProcessStartInfo(Environment.ProcessPath!, argument) { UseShellExecute = true, Verb = "runas" };
                using var p = Process.Start(psi);
                if (p == null) return false;
                p.WaitForExit();
                return p.ExitCode == 0;
            }
            catch (Win32Exception)
            {
                return false; // the user declined the UAC prompt
            }
        }

        static int Run(string file, string arguments)
        {
            var psi = new ProcessStartInfo(file, arguments)
            {
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
            };
            using var p = Process.Start(psi)!;
            p.StandardOutput.ReadToEnd();
            p.StandardError.ReadToEnd();
            p.WaitForExit();
            return p.ExitCode;
        }
    }
}
