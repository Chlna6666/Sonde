using System.Security.Cryptography;
using System.Text;

namespace Sonde.Internal;

internal static class Signing
{
    public const string SignatureVersion = "sonde-hmac-sha256-v2";

    public static string Sign(
        string signingKey,
        long timestampMs,
        string nonce,
        string method,
        string path,
        ReadOnlySpan<byte> body)
    {
        byte[] bodyHash = SHA256.HashData(body);
        string bodyHashHex = Convert.ToHexStringLower(bodyHash);
        string upperMethod = method.ToUpperInvariant();

        string canonical = $"sonde-hmac-sha256-v2\n{timestampMs}\n{nonce}\n{upperMethod}\n{path}\n{bodyHashHex}";
        byte[] canonicalBytes = Encoding.UTF8.GetBytes(canonical);
        byte[] keyBytes = Encoding.UTF8.GetBytes(signingKey);

        byte[] signature = HMACSHA256.HashData(keyBytes, canonicalBytes);
        return Convert.ToHexStringLower(signature);
    }
}
