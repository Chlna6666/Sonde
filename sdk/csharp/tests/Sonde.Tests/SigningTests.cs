using System.Security.Cryptography;
using System.Text;
using Sonde.Internal;

namespace Sonde.Tests;

public class SigningTests
{
    [Fact]
    public void SdkSignatureMatchesProtocolCanonicalization()
    {
        string key = "sec_test";
        long timestamp = 1_725_000_000_123L;
        string nonce = "018f47f2-2d4b-7d6c-9f30-111111111111";
        byte[] body = Encoding.UTF8.GetBytes("{\"items\":[{\"name\":\"app_startup\"}]}");

        string signature = Signing.Sign(key, timestamp, nonce, "POST", "/api/v1/ingest/events", body);

        // Compute expected signature directly
        byte[] bodyHash = SHA256.HashData(body);
        string bodyHashHex = Convert.ToHexStringLower(bodyHash);
        string canonical = $"sonde-hmac-sha256-v2\n{timestamp}\n{nonce}\nPOST\n/api/v1/ingest/events\n{bodyHashHex}";
        byte[] expectedHash = HMACSHA256.HashData(Encoding.UTF8.GetBytes(key), Encoding.UTF8.GetBytes(canonical));
        string expectedSignature = Convert.ToHexStringLower(expectedHash);

        Assert.Equal(expectedSignature, signature);
    }
}
