using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;
using System.Windows;
using BatteryTray.Core;
using Xunit;

namespace BatteryTray.Tests
{
    public sealed class OverlaySessionTests
    {
        [Fact]
        public void TimedPressRestartsTheFiveSecondDeadline()
        {
            var clock = new TestClock();
            var session = new OverlaySession(clock);
            Assert.False(session.IsVisible);
            session.Press(OverlayMode.Timed);
            Assert.True(session.IsTimed);
            clock.Advance(4);
            session.Expire();
            Assert.True(session.IsVisible);
            session.Press(OverlayMode.Timed);
            clock.Advance(4.9);
            session.Expire();
            Assert.True(session.IsVisible);
            clock.Advance(0.1);
            session.Expire();
            Assert.False(session.IsVisible);
        }

        [Fact]
        public void ToggleStaysVisibleUntilPressedAgain()
        {
            var clock = new TestClock();
            var session = new OverlaySession(clock);
            session.Press(OverlayMode.Toggle);
            Assert.False(session.IsTimed);
            clock.Advance(3600);
            session.Expire();
            Assert.True(session.IsVisible);
            session.Press(OverlayMode.Toggle);
            Assert.False(session.IsVisible);
        }

        [Fact]
        public void PreviewTimesOutEvenAfterAToggleSessionAndHideCancelsIt()
        {
            var clock = new TestClock();
            var session = new OverlaySession(clock);
            session.Press(OverlayMode.Toggle);
            session.Preview();
            clock.Advance(5);
            session.Expire();
            Assert.False(session.IsVisible);
            session.Preview();
            session.Hide();
            clock.Advance(1);
            session.Expire();
            Assert.False(session.IsVisible);
        }

        sealed class TestClock : TimeProvider
        {
            long timestamp;
            public override long TimestampFrequency => TimeSpan.TicksPerSecond;
            public override long GetTimestamp() => timestamp;
            public void Advance(double seconds) => timestamp += TimeSpan.FromSeconds(seconds).Ticks;
        }
    }

    public sealed class HotkeyTests
    {
        [Theory]
        [InlineData(0x56, 3, true)] // Ctrl+Alt+V
        [InlineData(0x74, 0, true)] // F5
        [InlineData(0x74, 1, true)] // Alt+F5
        [InlineData(0x87, 0, true)] // F24
        [InlineData(0x56, 0, false)]
        [InlineData(0x56, 8, false)] // Windows modifier
        [InlineData(0x7B, 3, false)] // F12
        [InlineData(0x11, 3, false)] // modifier only
        [InlineData(0xA0, 3, false)] // left Shift
        [InlineData(0x5B, 3, false)] // Windows key
        [InlineData(0, 3, false)]
        [InlineData(0xFF, 3, false)]
        [InlineData(0x56, -1, false)]
        public void ValidatesSupportedShortcuts(int key, int modifiers, bool valid) =>
            Assert.Equal(valid, new HotkeyGesture(key, modifiers).IsValid);

        [Fact]
        public void AConflictKeepsThePreviousRegistrationAndInvalidInputNeverRegisters()
        {
            var native = new Registration();
            using var binding = new HotkeyBinding(native);
            Assert.True(binding.TryChange(HotkeyGesture.Default));
            native.Fail = true;
            Assert.False(binding.TryChange(new HotkeyGesture(0x74, 0)));
            Assert.Equal(HotkeyGesture.Default, binding.Active);
            Assert.Empty(native.Removed);
            int calls = native.Registered.Count;
            Assert.False(binding.TryChange(new HotkeyGesture(0x7B, 3)));
            Assert.Equal(calls, native.Registered.Count);
        }

        [Fact]
        public void ReplacementRegistersBeforeReleasingOldIdAndIgnoresItsQueuedMessages()
        {
            var native = new Registration();
            var binding = new HotkeyBinding(native);
            Assert.True(binding.TryChange(HotkeyGesture.Default));
            Assert.True(binding.TryChange(new HotkeyGesture(0x74, 0)));
            Assert.Equal(new[] { "register:1", "register:2", "remove:1" }, native.Calls);
            Assert.False(binding.IsCurrent(1));
            Assert.True(binding.IsCurrent(2));
            Assert.True(binding.TryChange(new HotkeyGesture(0x74, 0)));
            Assert.Equal(2, native.Registered.Count);
            binding.Dispose();
            binding.Dispose();
            Assert.Equal(new[] { 1, 2 }, native.Removed);
            Assert.False(binding.IsCurrent(2));
        }

        sealed class Registration : IHotkeyRegistration
        {
            public bool Fail;
            public readonly List<int> Registered = new();
            public readonly List<int> Removed = new();
            public readonly List<string> Calls = new();
            public bool Register(int id, HotkeyGesture gesture)
            {
                Registered.Add(id);
                Calls.Add("register:" + id);
                return !Fail;
            }
            public void Unregister(int id) { Removed.Add(id); Calls.Add("remove:" + id); }
        }
    }

    public sealed class OverlaySettingsTests
    {
        [Fact]
        public void OldSettingsUseOverlayDefaultsAndRetainExistingValues()
        {
            var settings = JsonSerializer.Deserialize<AppSettings>("{\"LowPercent\":35,\"Sound\":false}")!;
            settings.NormalizeOverlay();
            Assert.Equal(35, settings.LowPercent);
            Assert.False(settings.Sound);
            Assert.True(settings.OverlayEnabled);
            Assert.Equal(OverlayMode.Timed, settings.OverlayMode);
            Assert.Equal(OverlayCorner.TopRight, settings.OverlayCorner);
            Assert.Equal(HotkeyGesture.Default, new HotkeyGesture(settings.OverlayHotkeyKey, settings.OverlayHotkeyModifiers));
            Assert.False(settings.OverlayShowNames);
        }

        [Fact]
        public void InvalidOverlayValuesResetIndependentlyOfOtherSettings()
        {
            var settings = new AppSettings
            {
                OverlayMode = (OverlayMode)99, OverlayCorner = (OverlayCorner)(-1),
                OverlayHotkeyKey = 0x7B, OverlayHotkeyModifiers = 8, OverlayEnabled = false, LowPercent = 40,
            };
            settings.NormalizeOverlay();
            Assert.Equal(OverlayMode.Timed, settings.OverlayMode);
            Assert.Equal(OverlayCorner.TopRight, settings.OverlayCorner);
            Assert.Equal(HotkeyGesture.Default.Key, settings.OverlayHotkeyKey);
            Assert.Equal(HotkeyGesture.Default.Modifiers, settings.OverlayHotkeyModifiers);
            Assert.False(settings.OverlayEnabled);
            Assert.Equal(40, settings.LowPercent);
        }

        [Fact]
        public void CustomOverlaySettingsRoundTrip()
        {
            var settings = new AppSettings
            {
                OverlayMode = OverlayMode.Toggle, OverlayCorner = OverlayCorner.BottomLeft,
                OverlayHotkeyKey = 0x74, OverlayHotkeyModifiers = 0, OverlayShowNames = true,
            };
            var copy = JsonSerializer.Deserialize<AppSettings>(JsonSerializer.Serialize(settings))!;
            copy.NormalizeOverlay();
            Assert.Equal(settings.OverlayMode, copy.OverlayMode);
            Assert.Equal(settings.OverlayCorner, copy.OverlayCorner);
            Assert.Equal(settings.OverlayHotkeyKey, copy.OverlayHotkeyKey);
            Assert.Equal(settings.OverlayHotkeyModifiers, copy.OverlayHotkeyModifiers);
            Assert.True(copy.OverlayShowNames);
        }
    }

    public sealed class OverlayDevicesTests
    {
        [Fact]
        public void FiltersOfflineAndHiddenDevicesWithoutUsingRememberedLevels()
        {
            var settings = new AppSettings();
            settings.For("hidden").Hidden = true;
            DeviceRow offline = Row("offline", DeviceKind.Mouse, online: false);
            DeviceRow remembered = offline with { Id = "remembered", Current = null };
            var result = OverlayDevices.Select(new[] { Row("shown", DeviceKind.Keyboard), offline, remembered,
                Row("hidden", DeviceKind.Mouse) }, settings);
            Assert.Equal("shown", Assert.Single(result).Device.Id);
        }

        [Fact]
        public void NamesAreShownOnlyForDuplicateTypesUnlessEnabledForAll()
        {
            var rows = new[] { Row("m1", DeviceKind.Mouse), Row("m2", DeviceKind.Mouse), Row("k", DeviceKind.Keyboard) };
            var settings = new AppSettings();
            var result = OverlayDevices.Select(rows, settings);
            Assert.All(result.Where(i => i.Device.Kind == DeviceKind.Mouse), i => Assert.True(i.ShowName));
            Assert.False(result.Single(i => i.Device.Kind == DeviceKind.Keyboard).ShowName);
            settings.OverlayShowNames = true;
            Assert.All(OverlayDevices.Select(rows, settings), i => Assert.True(i.ShowName));
        }

        [Fact]
        public void SortingDoesNotChangeWhenBatteryLevelsChangeAndNoDeviceCountIsHardcoded()
        {
            var rows = Enumerable.Range(0, 20).Select(i => Row(i.ToString("D2"), DeviceKind.Headset)).Reverse().ToArray();
            var settings = new AppSettings();
            string[] before = OverlayDevices.Select(rows, settings).Select(i => i.Device.Id).ToArray();
            var changed = rows.Select((r, i) => r with { Current = r.Current! with { Level = i } });
            Assert.Equal(before, OverlayDevices.Select(changed, settings).Select(i => i.Device.Id));
            Assert.Equal(20, before.Length);
        }

        [Fact]
        public void KeepsUnknownChargingAndEstimatedReadingsIntact()
        {
            var row = Row("charging", DeviceKind.Mouse);
            row = row with { Current = row.Current! with { Level = null, Charging = true, Estimate = true } };
            var result = Assert.Single(OverlayDevices.Select(new[] { row }, new AppSettings()));
            Assert.Same(row.Current, result.Device.Current);
        }

        static DeviceRow Row(string id, DeviceKind kind, bool online = true)
        {
            var reading = new DeviceReading { Id = id, Name = id, Kind = kind, Via = "test", Online = online, Level = 50 };
            return new DeviceRow(id, reading, reading, id);
        }
    }

    public sealed class OverlayPlacementTests
    {
        [Theory]
        [InlineData(OverlayCorner.TopLeft, -1908, 12)]
        [InlineData(OverlayCorner.TopRight, -212, 12)]
        [InlineData(OverlayCorner.BottomLeft, -1908, 1028)]
        [InlineData(OverlayCorner.BottomRight, -212, 1028)]
        public void AnchorsAllCornersOnANegativeCoordinateMonitor(OverlayCorner corner, double x, double y)
        {
            Assert.Equal(new Point(x, y), OverlayPlacement.Place(new Rect(-1920, 0, 1920, 1080), new Size(200, 40), corner, 12));
        }

        [Fact]
        public void AppliesScaledMarginAndAvoidsReminder()
        {
            var area = new Rect(0, 0, 1920, 1080);
            var reminder = new Rect(1490, 760, 420, 310);
            var size = new Size(400, 80);
            var point = OverlayPlacement.Place(area, size, OverlayCorner.BottomRight, 18, reminder);
            Assert.Equal(new Point(1502, 662), point);
            Assert.False(new Rect(point, size).IntersectsWith(reminder));
            Assert.True(area.Contains(new Rect(point, size)));
        }

        [Fact]
        public void OversizedContentAndTinyWorkAreasNeverProduceNegativeCoordinates()
        {
            var point = OverlayPlacement.Place(new Rect(0, 0, 10, 10), new Size(100, 100), OverlayCorner.BottomRight, 12);
            Assert.Equal(new Point(5, 5), point);
        }
    }
}
