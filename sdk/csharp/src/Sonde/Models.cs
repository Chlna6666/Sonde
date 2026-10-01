using System.Globalization;
using System.Runtime.InteropServices;
using System.Text.Json.Serialization;

namespace Sonde;

/// <summary>
/// Device facts reported by the client for platform and version analytics.
/// </summary>
public sealed class DeviceFacts
{
    [JsonPropertyName("appVersion")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? AppVersion { get; set; }

    [JsonPropertyName("os")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Os { get; set; }

    [JsonPropertyName("systemLanguage")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? SystemLanguage { get; set; }

    [JsonPropertyName("architecture")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Architecture { get; set; }

    public static DeviceFacts CreatePlatformDefaults()
    {
        string os = RuntimeInformation.IsOSPlatform(OSPlatform.Windows) ? "windows" :
                    RuntimeInformation.IsOSPlatform(OSPlatform.Linux) ? "linux" :
                    RuntimeInformation.IsOSPlatform(OSPlatform.OSX) ? "macos" : "unknown";

        string arch = RuntimeInformation.ProcessArchitecture switch
        {
            System.Runtime.InteropServices.Architecture.X64 => "x86_64",
            System.Runtime.InteropServices.Architecture.X86 => "x86",
            System.Runtime.InteropServices.Architecture.Arm64 => "aarch64",
            System.Runtime.InteropServices.Architecture.Arm => "arm",
            _ => RuntimeInformation.ProcessArchitecture.ToString().ToLowerInvariant()
        };

        return new DeviceFacts
        {
            Os = os,
            Architecture = arch,
            SystemLanguage = CultureInfo.CurrentUICulture.Name
        };
    }

    internal bool IsEmpty =>
        string.IsNullOrEmpty(AppVersion) &&
        string.IsNullOrEmpty(Os) &&
        string.IsNullOrEmpty(SystemLanguage) &&
        string.IsNullOrEmpty(Architecture);
}

/// <summary>
/// Represents a discrete telemetry event.
/// </summary>
public sealed class Event
{
    public Event(string name)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(name);
        Name = name;
    }

    [JsonPropertyName("name")]
    public string Name { get; set; }

    [JsonPropertyName("idempotencyKey")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? IdempotencyKey { get; set; }

    [JsonPropertyName("appVersion")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? AppVersion { get; set; }

    [JsonPropertyName("os")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Os { get; set; }

    [JsonPropertyName("systemLanguage")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? SystemLanguage { get; set; }

    [JsonPropertyName("architecture")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Architecture { get; set; }

    [JsonPropertyName("attributes")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingDefault)]
    public Dictionary<string, object?> Attributes { get; set; } = new();

    public Event WithAttribute(string key, object? value)
    {
        Attributes[key] = value;
        return this;
    }

    public Event WithIdempotencyKey(string key)
    {
        IdempotencyKey = key;
        return this;
    }

    public Event WithAppVersion(string version)
    {
        AppVersion = version;
        return this;
    }

    public Event WithOs(string os)
    {
        Os = os;
        return this;
    }

    public Event WithSystemLanguage(string language)
    {
        SystemLanguage = language;
        return this;
    }

    public Event WithArchitecture(string architecture)
    {
        Architecture = architecture;
        return this;
    }
}

[JsonConverter(typeof(JsonStringEnumConverter<MetricType>))]
public enum MetricType
{
    [JsonStringEnumMemberName("counter")]
    Counter,
    [JsonStringEnumMemberName("gauge")]
    Gauge,
    [JsonStringEnumMemberName("histogram")]
    Histogram
}

public sealed class Histogram
{
    [JsonPropertyName("count")]
    public ulong Count { get; set; }

    [JsonPropertyName("sum")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public double? Sum { get; set; }

    [JsonPropertyName("min")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public double? Min { get; set; }

    [JsonPropertyName("max")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public double? Max { get; set; }

    [JsonPropertyName("explicitBounds")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public List<double>? ExplicitBounds { get; set; }

    [JsonPropertyName("bucketCounts")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public List<ulong>? BucketCounts { get; set; }
}

/// <summary>
/// Metric telemetry for counters, gauges, and histograms.
/// </summary>
public sealed class Metric
{
    public Metric(string name, MetricType metricType)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(name);
        Name = name;
        MetricType = metricType;
    }

    [JsonPropertyName("name")]
    public string Name { get; set; }

    [JsonPropertyName("metricType")]
    public MetricType MetricType { get; set; }

    [JsonPropertyName("value")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public double? Value { get; set; }

    [JsonPropertyName("histogram")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public Histogram? Histogram { get; set; }

    [JsonPropertyName("unit")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Unit { get; set; }

    [JsonPropertyName("attributes")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingDefault)]
    public Dictionary<string, object?> Attributes { get; set; } = new();

    public static Metric Counter(string name, double value) => new(name, MetricType.Counter) { Value = value };
    public static Metric Gauge(string name, double value) => new(name, MetricType.Gauge) { Value = value };
    public static Metric FromHistogram(string name, Histogram histogram) => new(name, MetricType.Histogram) { Histogram = histogram };

    public Metric WithUnit(string unit)
    {
        Unit = unit;
        return this;
    }

    public Metric WithAttribute(string key, object? value)
    {
        Attributes[key] = value;
        return this;
    }
}

[JsonConverter(typeof(JsonStringEnumConverter<LogLevel>))]
public enum LogLevel
{
    [JsonStringEnumMemberName("trace")]
    Trace,
    [JsonStringEnumMemberName("debug")]
    Debug,
    [JsonStringEnumMemberName("info")]
    Info,
    [JsonStringEnumMemberName("warn")]
    Warn,
    [JsonStringEnumMemberName("error")]
    Error,
    [JsonStringEnumMemberName("fatal")]
    Fatal
}

/// <summary>
/// Structured application log entry.
/// </summary>
public sealed class LogEntry
{
    public LogEntry(LogLevel level, string message)
    {
        Level = level;
        Message = message ?? string.Empty;
    }

    [JsonPropertyName("level")]
    public LogLevel Level { get; set; }

    [JsonPropertyName("message")]
    public string Message { get; set; }

    [JsonPropertyName("logger")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Logger { get; set; }

    [JsonPropertyName("traceId")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? TraceId { get; set; }

    [JsonPropertyName("spanId")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? SpanId { get; set; }

    [JsonPropertyName("attributes")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingDefault)]
    public Dictionary<string, object?> Attributes { get; set; } = new();

    public LogEntry WithLogger(string logger)
    {
        Logger = logger;
        return this;
    }

    public LogEntry WithAttribute(string key, object? value)
    {
        Attributes[key] = value;
        return this;
    }
}

[JsonConverter(typeof(JsonStringEnumConverter<ErrorSeverity>))]
public enum ErrorSeverity
{
    [JsonStringEnumMemberName("fatal")]
    Fatal,
    [JsonStringEnumMemberName("error")]
    Error,
    [JsonStringEnumMemberName("warning")]
    Warning
}

/// <summary>
/// Error occurrence event with stack trace and context.
/// </summary>
public sealed class ErrorEvent
{
    public ErrorEvent(string name, string message)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(name);
        Name = name;
        Message = message ?? string.Empty;
    }

    [JsonPropertyName("name")]
    public string Name { get; set; }

    [JsonPropertyName("message")]
    public string Message { get; set; }

    [JsonPropertyName("stackTrace")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? StackTrace { get; set; }

    [JsonPropertyName("severity")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public ErrorSeverity? Severity { get; set; }

    [JsonPropertyName("handled")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public bool? Handled { get; set; }

    [JsonPropertyName("appVersion")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? AppVersion { get; set; }

    [JsonPropertyName("os")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Os { get; set; }

    [JsonPropertyName("systemLanguage")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? SystemLanguage { get; set; }

    [JsonPropertyName("architecture")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Architecture { get; set; }

    [JsonPropertyName("attributes")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingDefault)]
    public Dictionary<string, object?> Attributes { get; set; } = new();

    public static ErrorEvent FromException(Exception exception, bool handled = false)
    {
        ArgumentNullException.ThrowIfNull(exception);
        return new ErrorEvent(exception.GetType().FullName ?? exception.GetType().Name, exception.Message)
        {
            StackTrace = exception.StackTrace,
            Severity = ErrorSeverity.Error,
            Handled = handled
        };
    }

    public ErrorEvent WithSeverity(ErrorSeverity severity)
    {
        Severity = severity;
        return this;
    }

    public ErrorEvent WithStackTrace(string stackTrace)
    {
        StackTrace = stackTrace;
        return this;
    }

    public ErrorEvent WithHandled(bool handled)
    {
        Handled = handled;
        return this;
    }

    public ErrorEvent WithAttribute(string key, object? value)
    {
        Attributes[key] = value;
        return this;
    }
}

internal sealed class BatchEnvelope<T>
{
    [JsonPropertyName("items")]
    public required IReadOnlyList<T> Items { get; init; }
}

internal sealed class TokenRequest
{
    [JsonPropertyName("deviceId")]
    public required string DeviceId { get; init; }
}

internal sealed class TokenResponse
{
    [JsonPropertyName("token")]
    public required string Token { get; init; }

    [JsonPropertyName("signingKey")]
    public required string SigningKey { get; init; }

    [JsonPropertyName("expiresAt")]
    public required long ExpiresAt { get; init; }

    [JsonPropertyName("signatureVersion")]
    public required string SignatureVersion { get; init; }
}

public sealed class BatchReceipt
{
    [JsonPropertyName("accepted")]
    public int Accepted { get; set; }

    [JsonPropertyName("rejected")]
    public List<RejectedItem> Rejected { get; set; } = new();
}

public sealed class RejectedItem
{
    [JsonPropertyName("index")]
    public int Index { get; set; }

    [JsonPropertyName("reason")]
    public string Reason { get; set; } = string.Empty;
}
