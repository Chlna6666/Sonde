using Sonde.Internal;

namespace Sonde;

/// <summary>
/// Sonde telemetry client providing authenticated, buffered and asynchronous telemetry ingest.
/// </summary>
public sealed class SondeClient : IAsyncDisposable, IDisposable
{
    private readonly Transport _transport;
    private readonly DeliveryQueue<Event> _eventQueue;
    private readonly DeliveryQueue<Metric> _metricQueue;
    private readonly DeliveryQueue<LogEntry> _logQueue;
    private readonly DeliveryQueue<ErrorEvent> _errorQueue;
    private readonly CancellationTokenSource _cts = new();
    private readonly Task? _heartbeatTask;

    private DeviceFacts _facts;
    private bool _disposed;

    private SondeClient(
        Transport transport,
        DeviceFacts facts,
        TimeSpan? heartbeatInterval,
        DeliveryOptions deliveryOptions)
    {
        _transport = transport;
        _facts = facts;

        _eventQueue = new DeliveryQueue<Event>(transport, "/events", deliveryOptions);
        _metricQueue = new DeliveryQueue<Metric>(transport, "/metrics", deliveryOptions);
        _logQueue = new DeliveryQueue<LogEntry>(transport, "/logs", deliveryOptions);
        _errorQueue = new DeliveryQueue<ErrorEvent>(transport, "/errors", deliveryOptions);

        if (heartbeatInterval is { } interval && interval > TimeSpan.Zero)
        {
            _heartbeatTask = Task.Run(() => HeartbeatLoopAsync(interval, _cts.Token));
        }
    }

    /// <summary>
    /// Gets the resolved pseudonym device identifier used by this client instance.
    /// </summary>
    public string DeviceId => _transport.DeviceId;

    /// <summary>
    /// Gets the normalized ingest endpoint URI.
    /// </summary>
    public string Endpoint => _transport.Endpoint;

    /// <summary>
    /// Connects to a Sonde telemetry server using the specified options.
    /// </summary>
    public static async Task<SondeClient> ConnectAsync(SondeOptions options, CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(options);
        ArgumentException.ThrowIfNullOrWhiteSpace(options.BaseUrl);
        ArgumentException.ThrowIfNullOrWhiteSpace(options.ApiKey);

        string deviceId = !string.IsNullOrWhiteSpace(options.DeviceId)
            ? Sonde.DeviceId.ValidateDeviceId(options.DeviceId)
            : Sonde.DeviceId.MachineDeviceId();

        var transport = new Transport(
            options.BaseUrl,
            options.ApiKey,
            deviceId,
            options.UserAgent,
            options.RequestTimeout,
            options.HttpClient);

        // Fail fast on token acquisition / auth
        await transport.GetTokenAsync(cancellationToken).ConfigureAwait(false);

        // Initial heartbeat (resilient to retryable errors)
        try
        {
            await transport.HeartbeatAsync(options.Facts, cancellationToken).ConfigureAwait(false);
        }
        catch (SondeException ex) when (ex.IsRetryable)
        {
            // Allowed to proceed with queues
        }

        return new SondeClient(transport, options.Facts, options.HeartbeatInterval, options.Delivery);
    }

    /// <summary>
    /// Connects to a Sonde telemetry server using a machine-derived identifier.
    /// </summary>
    public static Task<SondeClient> FromMachineAsync(
        string baseUrl,
        string apiKey,
        string? salt = null,
        CancellationToken cancellationToken = default)
    {
        var options = new SondeOptions
        {
            BaseUrl = baseUrl,
            ApiKey = apiKey,
            DeviceId = Sonde.DeviceId.MachineDeviceId(salt)
        };
        return ConnectAsync(options, cancellationToken);
    }

    /// <summary>
    /// Enqueues a telemetry event in the non-blocking background queue.
    /// </summary>
    public void TrackEvent(Event @event)
    {
        EnsureNotDisposed();
        EnrichEvent(@event);
        _eventQueue.TryEnqueue(@event);
    }

    /// <summary>
    /// Enqueues a telemetry event asynchronously.
    /// </summary>
    public async ValueTask TrackEventAsync(Event @event, CancellationToken cancellationToken = default)
    {
        EnsureNotDisposed();
        EnrichEvent(@event);
        await _eventQueue.EnqueueAsync(@event, cancellationToken).ConfigureAwait(false);
    }

    /// <summary>
    /// Enqueues a metric in the non-blocking background queue.
    /// </summary>
    public void TrackMetric(Metric metric)
    {
        EnsureNotDisposed();
        _metricQueue.TryEnqueue(metric);
    }

    /// <summary>
    /// Enqueues a metric asynchronously.
    /// </summary>
    public async ValueTask TrackMetricAsync(Metric metric, CancellationToken cancellationToken = default)
    {
        EnsureNotDisposed();
        await _metricQueue.EnqueueAsync(metric, cancellationToken).ConfigureAwait(false);
    }

    /// <summary>
    /// Enqueues a log entry in the non-blocking background queue.
    /// </summary>
    public void TrackLog(LogEntry log)
    {
        EnsureNotDisposed();
        _logQueue.TryEnqueue(log);
    }

    /// <summary>
    /// Enqueues a log entry asynchronously.
    /// </summary>
    public async ValueTask TrackLogAsync(LogEntry log, CancellationToken cancellationToken = default)
    {
        EnsureNotDisposed();
        await _logQueue.EnqueueAsync(log, cancellationToken).ConfigureAwait(false);
    }

    /// <summary>
    /// Enqueues an error event in the non-blocking background queue.
    /// </summary>
    public void TrackError(ErrorEvent error)
    {
        EnsureNotDisposed();
        EnrichError(error);
        _errorQueue.TryEnqueue(error);
    }

    /// <summary>
    /// Enqueues an error event asynchronously.
    /// </summary>
    public async ValueTask TrackErrorAsync(ErrorEvent error, CancellationToken cancellationToken = default)
    {
        EnsureNotDisposed();
        EnrichError(error);
        await _errorQueue.EnqueueAsync(error, cancellationToken).ConfigureAwait(false);
    }

    /// <summary>
    /// Manually sends a heartbeat immediately.
    /// </summary>
    public async Task HeartbeatAsync(CancellationToken cancellationToken = default)
    {
        EnsureNotDisposed();
        await _transport.HeartbeatAsync(_facts, cancellationToken).ConfigureAwait(false);
    }

    /// <summary>
    /// Updates the current device facts and immediately transmits a heartbeat.
    /// </summary>
    public async Task SetDeviceFactsAsync(DeviceFacts facts, CancellationToken cancellationToken = default)
    {
        EnsureNotDisposed();
        ArgumentNullException.ThrowIfNull(facts);
        _facts = facts;
        await _transport.HeartbeatAsync(_facts, cancellationToken).ConfigureAwait(false);
    }

    /// <summary>
    /// Flushes all queued events, metrics, logs, and errors to the server.
    /// </summary>
    public async Task FlushAsync(CancellationToken cancellationToken = default)
    {
        EnsureNotDisposed();
        await Task.WhenAll(
            _eventQueue.FlushAsync(cancellationToken),
            _metricQueue.FlushAsync(cancellationToken),
            _logQueue.FlushAsync(cancellationToken),
            _errorQueue.FlushAsync(cancellationToken)
        ).ConfigureAwait(false);
    }

    private void EnrichEvent(Event e)
    {
        e.AppVersion ??= _facts.AppVersion;
        e.Os ??= _facts.Os;
        e.SystemLanguage ??= _facts.SystemLanguage;
        e.Architecture ??= _facts.Architecture;
    }

    private void EnrichError(ErrorEvent e)
    {
        e.AppVersion ??= _facts.AppVersion;
        e.Os ??= _facts.Os;
        e.SystemLanguage ??= _facts.SystemLanguage;
        e.Architecture ??= _facts.Architecture;
    }

    private async Task HeartbeatLoopAsync(TimeSpan interval, CancellationToken cancellationToken)
    {
        using var timer = new PeriodicTimer(interval);
        try
        {
            while (await timer.WaitForNextTickAsync(cancellationToken).ConfigureAwait(false))
            {
                try
                {
                    await _transport.HeartbeatAsync(_facts, cancellationToken).ConfigureAwait(false);
                }
                catch
                {
                }
            }
        }
        catch (OperationCanceledException)
        {
        }
    }

    private void EnsureNotDisposed()
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
    }

    public async ValueTask DisposeAsync()
    {
        if (_disposed) return;
        _disposed = true;

        _cts.Cancel();
        if (_heartbeatTask != null)
        {
            try
            {
                await _heartbeatTask.ConfigureAwait(false);
            }
            catch
            {
            }
        }

        await _eventQueue.DisposeAsync().ConfigureAwait(false);
        await _metricQueue.DisposeAsync().ConfigureAwait(false);
        await _logQueue.DisposeAsync().ConfigureAwait(false);
        await _errorQueue.DisposeAsync().ConfigureAwait(false);

        _transport.Dispose();
        _cts.Dispose();
    }

    public void Dispose()
    {
        DisposeAsync().AsTask().GetAwaiter().GetResult();
    }
}
