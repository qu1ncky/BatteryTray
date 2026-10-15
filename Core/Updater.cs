using System;
using System.Diagnostics;
using System.IO;
using System.Net.Http;
using System.Reflection;
using System.Security.Cryptography;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;

namespace BatteryTray.Core
{
    /// <summary>The newest published release and its exe asset.</summary>
    public sealed record ReleaseInfo(Version Version, string Tag, string Notes, string PageUrl, string ExeUrl, long ExeSize,
        string Sha256);

    /// <summary>
    /// Updates from GitHub Releases: the "BatteryTray.exe" asset of the latest release replaces the running exe.
    /// The new exe is downloaded straight into the exe's own folder, so an install in Program Files never passes
    /// through a folder other programs can write to. It must match the SHA-256 digest GitHub publishes for the asset
    /// and carry the release's version (a mislabeled asset would otherwise update forever). Then it is swapped in:
    /// the running exe is renamed to *.old.exe (Windows allows renaming a running exe) and the new one takes its name.
    /// The old file is deleted on the next start.
    /// </summary>
    public static class Updater
    {
        public const string Repository = "qu1ncky/BatteryTray";
        const string AssetName = "BatteryTray.exe";

        static readonly HttpClient Http = CreateClient();

        static HttpClient CreateClient()
        {
            var client = new HttpClient { Timeout = TimeSpan.FromMinutes(10) };
            client.DefaultRequestHeaders.UserAgent.ParseAdd($"BatteryTray/{Current}"); // GitHub's API requires one
            return client;
        }

        public static Version Current
        {
            get
            {
                Version? v = Assembly.GetExecutingAssembly().GetName().Version;
                return v == null ? new Version(0, 0, 0) : new Version(v.Major, v.Minor, Math.Max(v.Build, 0));
            }
        }

        static string Exe => Environment.ProcessPath ?? throw new InvalidOperationException("no process path");

        static string Sibling(string suffix) =>
            Path.Combine(Path.GetDirectoryName(Exe)!, Path.GetFileNameWithoutExtension(Exe) + suffix);

        /// <summary>The latest non-prerelease release, or null when it has no usable exe asset.</summary>
        public static async Task<ReleaseInfo?> GetLatestAsync(CancellationToken ct = default)
        {
            using var request = new HttpRequestMessage(HttpMethod.Get, $"https://api.github.com/repos/{Repository}/releases/latest");
            request.Headers.Accept.ParseAdd("application/vnd.github+json");
            using var response = await Http.SendAsync(request, ct).ConfigureAwait(false);
            response.EnsureSuccessStatusCode();
            using var doc = JsonDocument.Parse(await response.Content.ReadAsStringAsync(ct).ConfigureAwait(false));
            JsonElement root = doc.RootElement;

            string tag = root.GetProperty("tag_name").GetString() ?? "";
            if (!Version.TryParse(tag.TrimStart('v', 'V'), out Version? version)) return null;
            foreach (JsonElement asset in root.GetProperty("assets").EnumerateArray())
            {
                if (asset.GetProperty("name").GetString() != AssetName) continue;
                string? digest = asset.TryGetProperty("digest", out var d) ? d.GetString() : null;
                if (digest == null || !digest.StartsWith("sha256:", StringComparison.OrdinalIgnoreCase)) return null;
                return new ReleaseInfo(
                    new Version(version.Major, version.Minor, Math.Max(version.Build, 0)), tag,
                    root.TryGetProperty("body", out var body) ? body.GetString() ?? "" : "",
                    root.GetProperty("html_url").GetString() ?? "",
                    asset.GetProperty("browser_download_url").GetString() ?? "",
                    asset.GetProperty("size").GetInt64(),
                    digest["sha256:".Length..].ToLowerInvariant());
            }
            return null;
        }

        /// <summary>True when this process may replace its own exe: a portable copy, or an elevated one in Program
        /// Files. Otherwise an elevated copy of the exe has to do it (<c>--update</c>).</summary>
        public static bool CanInstallInPlace()
        {
            string probe = Sibling(".write-test");
            try
            {
                File.WriteAllText(probe, "");
                File.Delete(probe);
                return true;
            }
            catch (Exception)
            {
                return false;
            }
        }

        public static async Task InstallAsync(ReleaseInfo release, IProgress<int>? progress, CancellationToken ct = default)
        {
            string exe = Exe;
            string fresh = Sibling(".new.exe");
            string old = Sibling(".old.exe");
            CleanUp();

            await DownloadAsync(release, fresh, progress, ct).ConfigureAwait(false);
            try
            {
                Verify(fresh, release);
            }
            catch
            {
                File.Delete(fresh);
                throw;
            }

            File.Move(exe, old);
            try
            {
                File.Move(fresh, exe);
            }
            catch
            {
                File.Move(old, exe); // put the working version back
                throw;
            }
            Log.Write($"update: {Current} -> {release.Version} installed at {exe}");
        }

        /// <summary>Removes what a previous update left behind (the old exe can only go once it has exited).</summary>
        public static void CleanUp()
        {
            foreach (string leftover in new[] { Sibling(".old.exe"), Sibling(".new.exe") })
            {
                try
                {
                    if (File.Exists(leftover)) File.Delete(leftover);
                }
                catch (Exception)
                {
                    // Still running (the previous version is exiting): next start removes it
                }
            }
        }

        static async Task DownloadAsync(ReleaseInfo release, string path, IProgress<int>? progress, CancellationToken ct)
        {
            using var response = await Http.GetAsync(release.ExeUrl, HttpCompletionOption.ResponseHeadersRead, ct)
                .ConfigureAwait(false);
            response.EnsureSuccessStatusCode();
            await using var input = await response.Content.ReadAsStreamAsync(ct).ConfigureAwait(false);
            await using var output = new FileStream(path, FileMode.Create, FileAccess.Write, FileShare.None, 1 << 16, true);
            var buffer = new byte[1 << 16];
            long done = 0;
            int lastPercent = -1;
            int n;
            while ((n = await input.ReadAsync(buffer, ct).ConfigureAwait(false)) > 0)
            {
                await output.WriteAsync(buffer.AsMemory(0, n), ct).ConfigureAwait(false);
                done += n;
                int percent = (int)(done * 100 / Math.Max(release.ExeSize, 1));
                if (percent != lastPercent) progress?.Report(lastPercent = percent);
            }
        }

        static void Verify(string path, ReleaseInfo release)
        {
            long size = new FileInfo(path).Length;
            if (size != release.ExeSize) throw new InvalidDataException($"размер файла {size} вместо {release.ExeSize}");

            string hash;
            using (var stream = File.OpenRead(path))
                hash = Convert.ToHexString(SHA256.HashData(stream)).ToLowerInvariant();
            if (hash != release.Sha256) throw new InvalidDataException("контрольная сумма не совпала");

            string? fileVersion = FileVersionInfo.GetVersionInfo(path).FileVersion;
            if (!Version.TryParse(fileVersion, out Version? v)
                || new Version(v.Major, v.Minor, Math.Max(v.Build, 0)) != release.Version)
                throw new InvalidDataException($"в релизе {release.Tag} лежит exe версии {fileVersion}");
        }
    }
}
