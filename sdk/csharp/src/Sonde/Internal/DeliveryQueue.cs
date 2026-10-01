using System.Threading.Channels;

namespace Sonde.Internal;

internal abstract class QueueMessage<T>
{
    public sealed class ItemMessage(T item) : QueueMessage<T>
    {
        public T Item => item;
    }

    public sealed class FlushMessage(TaskCompletionSource tcs) : QueueMessage<T>
    {
        public TaskCompletionSource Tcs => tcs;
    }
}

internal sealed class DeliveryQueue<T> : IAsyncDisposable
{
    private readonly Transport _transport;
    private readonly string _route;
    private readonly DeliveryOptions _options;
    private readonly Channel<QueueMessage<T>> _channel;
    private readonly Task _workerTask;
    private readonly CancellationTokenSource _cts = new();

    public DeliveryQueue(Transport transport, string route, DeliveryOptions options)
    {
        _transport = transport;
        _route = route;
        _options = options;

        var channelOptions = new BoundedChannelOptions(options.QueueCapacity)
        {
            FullMode = BoundedChannelFullMode.DropOldest,
            SingleReader = true
        };
        _channel = Channel.CreateBounded<QueueMessage<T>>(channelOptions);
        _workerTask = Task.Run(ProcessQueueAsync);
    }

    public bool TryEnqueue(T item) => _channel.Writer.TryWrite(new QueueMessage<T>.ItemMessage(item));

    public ValueTask EnqueueAsync(T item, CancellationToken cancellationToken = default)
    {
        return _channel.Writer.WriteAsync(new QueueMessage<T>.ItemMessage(item), cancellationToken);
    }

    public async Task FlushAsync(CancellationToken cancellationToken = default)
    {
        var tcs = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        await _channel.Writer.WriteAsync(new QueueMessage<T>.FlushMessage(tcs), cancellationToken).ConfigureAwait(false);
        await tcs.Task.WaitAsync(cancellationToken).ConfigureAwait(false);
    }

    private async Task ProcessQueueAsync()
    {
        var buffer = new List<T>(_options.MaxBatchItems);
        using var timer = new PeriodicTimer(_options.FlushInterval);

        try
        {
            while (!_cts.IsCancellationRequested)
            {
                var readWaitTask = _channel.Reader.WaitToReadAsync(_cts.Token).AsTask();
                var timerTickTask = timer.WaitForNextTickAsync(_cts.Token).AsTask();

                var completed = await Task.WhenAny(readWaitTask, timerTickTask).ConfigureAwait(false);

                while (_channel.Reader.TryRead(out var msg))
                {
                    if (msg is QueueMessage<T>.ItemMessage itemMsg)
                    {
                        buffer.Add(itemMsg.Item);
                        if (buffer.Count >= _options.MaxBatchItems)
                        {
                            await SendWithRetryAsync(buffer, _cts.Token).ConfigureAwait(false);
                            buffer.Clear();
                        }
                    }
                    else if (msg is QueueMessage<T>.FlushMessage flushMsg)
                    {
                        if (buffer.Count > 0)
                        {
                            await SendWithRetryAsync(buffer, _cts.Token).ConfigureAwait(false);
                            buffer.Clear();
                        }
                        flushMsg.Tcs.TrySetResult();
                    }
                }

                if (completed == timerTickTask && buffer.Count > 0)
                {
                    await SendWithRetryAsync(buffer, _cts.Token).ConfigureAwait(false);
                    buffer.Clear();
                }

                if (completed == readWaitTask && !await readWaitTask.ConfigureAwait(false))
                {
                    break;
                }
            }
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception)
        {
        }

        // Drain on shutdown
        while (_channel.Reader.TryRead(out var msg))
        {
            if (msg is QueueMessage<T>.ItemMessage itemMsg)
            {
                buffer.Add(itemMsg.Item);
                if (buffer.Count >= _options.MaxBatchItems)
                {
                    try
                    {
                        using var shutdownCts = new CancellationTokenSource(TimeSpan.FromSeconds(5));
                        await SendWithRetryAsync(buffer, shutdownCts.Token).ConfigureAwait(false);
                    }
                    catch
                    {
                    }
                    buffer.Clear();
                }
            }
            else if (msg is QueueMessage<T>.FlushMessage flushMsg)
            {
                if (buffer.Count > 0)
                {
                    try
                    {
                        using var shutdownCts = new CancellationTokenSource(TimeSpan.FromSeconds(5));
                        await SendWithRetryAsync(buffer, shutdownCts.Token).ConfigureAwait(false);
                    }
                    catch
                    {
                    }
                    buffer.Clear();
                }
                flushMsg.Tcs.TrySetResult();
            }
        }

        if (buffer.Count > 0)
        {
            try
            {
                using var shutdownCts = new CancellationTokenSource(TimeSpan.FromSeconds(5));
                await SendWithRetryAsync(buffer, shutdownCts.Token).ConfigureAwait(false);
            }
            catch
            {
            }
        }
    }

    private async Task SendWithRetryAsync(IReadOnlyList<T> items, CancellationToken cancellationToken)
    {
        int attempts = 0;
        TimeSpan delay = _options.Retry.InitialBackoff;

        while (true)
        {
            cancellationToken.ThrowIfCancellationRequested();
            try
            {
                await _transport.SendBatchAsync(_route, items, cancellationToken).ConfigureAwait(false);
                return;
            }
            catch (SondeException ex) when (ex.IsRetryable && attempts < _options.Retry.MaxRetries)
            {
                attempts++;
                TimeSpan waitTime = ex.RetryAfter ?? delay;
                await Task.Delay(waitTime, cancellationToken).ConfigureAwait(false);
                delay = TimeSpan.FromMilliseconds(Math.Min(delay.TotalMilliseconds * 2, _options.Retry.MaxBackoff.TotalMilliseconds));
            }
            catch (Exception) when (attempts >= _options.Retry.MaxRetries)
            {
                return;
            }
        }
    }

    public async ValueTask DisposeAsync()
    {
        _channel.Writer.TryComplete();
        _cts.Cancel();
        try
        {
            await _workerTask.ConfigureAwait(false);
        }
        catch
        {
        }
        _cts.Dispose();
    }
}
