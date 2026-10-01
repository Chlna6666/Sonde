using System.Text.Json;

namespace Sonde.Tests;

public class ModelSerializationTests
{
    [Fact]
    public void EventSerializesToCamelCase()
    {
        var @event = new Event("app_startup")
            .WithAppVersion("1.0.0")
            .WithOs("windows")
            .WithAttribute("source", "launcher");

        string json = JsonSerializer.Serialize(@event);

        using var doc = JsonDocument.Parse(json);
        var root = doc.RootElement;
        Assert.Equal("app_startup", root.GetProperty("name").GetString());
        Assert.Equal("1.0.0", root.GetProperty("appVersion").GetString());
        Assert.Equal("windows", root.GetProperty("os").GetString());
        Assert.Equal("launcher", root.GetProperty("attributes").GetProperty("source").GetString());
    }

    [Fact]
    public void MetricSerializesCorrectly()
    {
        var counter = Metric.Counter("requests_total", 42);
        string json = JsonSerializer.Serialize(counter);

        using var doc = JsonDocument.Parse(json);
        var root = doc.RootElement;
        Assert.Equal("requests_total", root.GetProperty("name").GetString());
        Assert.Equal("counter", root.GetProperty("metricType").GetString());
        Assert.Equal(42.0, root.GetProperty("value").GetDouble());
    }

    [Fact]
    public void LogEntrySerializesLevelInLowercase()
    {
        var log = new LogEntry(LogLevel.Warn, "Disk space low")
            .WithLogger("storage");

        string json = JsonSerializer.Serialize(log);

        using var doc = JsonDocument.Parse(json);
        var root = doc.RootElement;
        Assert.Equal("warn", root.GetProperty("level").GetString());
        Assert.Equal("Disk space low", root.GetProperty("message").GetString());
        Assert.Equal("storage", root.GetProperty("logger").GetString());
    }

    [Fact]
    public void ErrorEventSerializesFromException()
    {
        try
        {
            throw new InvalidOperationException("Something went wrong");
        }
        catch (Exception ex)
        {
            var error = ErrorEvent.FromException(ex, handled: true);
            string json = JsonSerializer.Serialize(error);

            using var doc = JsonDocument.Parse(json);
            var root = doc.RootElement;
            Assert.Equal("System.InvalidOperationException", root.GetProperty("name").GetString());
            Assert.Equal("Something went wrong", root.GetProperty("message").GetString());
            Assert.True(root.GetProperty("handled").GetBoolean());
            Assert.True(root.TryGetProperty("stackTrace", out _));
        }
    }
}
