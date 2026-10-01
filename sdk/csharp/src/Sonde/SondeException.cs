using System.Net;

namespace Sonde;

public class SondeException : Exception
{
    public HttpStatusCode? StatusCode { get; }
    public bool IsRetryable { get; }
    public TimeSpan? RetryAfter { get; }

    public SondeException(string message) : base(message)
    {
    }

    public SondeException(string message, Exception innerException) : base(message, innerException)
    {
        IsRetryable = innerException is HttpRequestException or TaskCanceledException or TimeoutException;
    }

    public SondeException(HttpStatusCode statusCode, string message, TimeSpan? retryAfter = null) : base($"Sonde returned HTTP {(int)statusCode}: {message}")
    {
        StatusCode = statusCode;
        RetryAfter = retryAfter;
        IsRetryable = statusCode is HttpStatusCode.TooManyRequests || (int)statusCode >= 500;
    }
}
