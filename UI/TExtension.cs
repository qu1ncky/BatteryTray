using System;
using System.Windows.Data;
using System.Windows.Markup;
using BatteryTray.Core;

namespace BatteryTray.UI
{
    /// <summary><c>{l:T key}</c> in XAML: the text for the key in the current language, updated live when the
    /// language changes.</summary>
    [MarkupExtensionReturnType(typeof(object))]
    public sealed class TExtension : MarkupExtension
    {
        public TExtension(string key)
        {
            Key = key;
        }

        [ConstructorArgument("key")]
        public string Key { get; set; }

        public override object ProvideValue(IServiceProvider serviceProvider)
        {
            return new Binding($"[{Key}]") { Source = Loc.Instance, Mode = BindingMode.OneWay }.ProvideValue(serviceProvider);
        }
    }
}
