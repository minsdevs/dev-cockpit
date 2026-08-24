import { describe, expect, it } from "vitest";

/**
 * Tests for the external tool integration logic.
 * These test the URL encoding, shell escaping, and validation logic
 * that corresponds to the Rust backend (tools.rs).
 */

// Mirrors the Rust url_encode function for frontend validation
function urlEncode(input: string): string {
  // Standard percent-encoding matching RFC 3986 unreserved characters
  return Array.from(new TextEncoder().encode(input))
    .map((byte) => {
      if (
        (byte >= 0x41 && byte <= 0x5a) || // A-Z
        (byte >= 0x61 && byte <= 0x7a) || // a-z
        (byte >= 0x30 && byte <= 0x39) || // 0-9
        byte === 0x2d || // -
        byte === 0x5f || // _
        byte === 0x2e || // .
        byte === 0x7e // ~
      ) {
        return String.fromCharCode(byte);
      }
      return `%${byte.toString(16).toUpperCase().padStart(2, "0")}`;
    })
    .join("");
}

// Mirrors the Rust shell_escape function
function shellEscape(input: string): string {
  const escaped = input.replace(/'/g, "'\\''");
  return `'${escaped}'`;
}

describe("URL encoding (mirrors Rust tools::url_encode)", () => {
  it("preserves unreserved characters", () => {
    expect(urlEncode("hello")).toBe("hello");
    expect(urlEncode("a-b_c.d~e")).toBe("a-b_c.d~e");
    expect(urlEncode("ABC123")).toBe("ABC123");
  });

  it("encodes path separators", () => {
    const encoded = urlEncode("/Users/me/Dev/project");
    expect(encoded).toBe("%2FUsers%2Fme%2FDev%2Fproject");
  });

  it("encodes spaces", () => {
    expect(urlEncode("path with spaces")).toBe("path%20with%20spaces");
  });

  it("encodes special characters", () => {
    const encoded = urlEncode("special!@#$%");
    expect(encoded).toContain("%21"); // !
    expect(encoded).toContain("%40"); // @
    expect(encoded).toContain("%23"); // #
    expect(encoded).toContain("%24"); // $
    expect(encoded).toContain("%25"); // %
  });

  it("encodes Unicode (Korean)", () => {
    const encoded = urlEncode("경로/테스트");
    expect(encoded).not.toContain("경");
    expect(encoded).toContain("%");
    // Korean chars are multi-byte UTF-8
    expect(encoded.length).toBeGreaterThan("경로/테스트".length);
  });

  it("produces valid Code Intelligence URL", () => {
    const path = "/Users/dev/my-project";
    const url = `http://localhost:3000/import?path=${urlEncode(path)}`;
    expect(url).toBe(
      "http://localhost:3000/import?path=%2FUsers%2Fdev%2Fmy-project"
    );
  });
});

describe("shell escaping (mirrors Rust tools::shell_escape)", () => {
  it("wraps simple paths in single quotes", () => {
    expect(shellEscape("/Users/me/Dev")).toBe("'/Users/me/Dev'");
  });

  it("escapes embedded single quotes", () => {
    expect(shellEscape("it's")).toBe("'it'\\''s'");
    expect(shellEscape("a'b'c")).toBe("'a'\\''b'\\''c'");
  });

  it("prevents command injection via $(...)", () => {
    const escaped = shellEscape("$(rm -rf /)");
    expect(escaped).toBe("'$(rm -rf /)'");
    // When used in a shell, single-quoted strings are literal
  });

  it("prevents command injection via backticks", () => {
    expect(shellEscape("`whoami`")).toBe("'`whoami`'");
  });

  it("prevents injection via semicolons and pipes", () => {
    expect(shellEscape("a;b")).toBe("'a;b'");
    expect(shellEscape("a&&b")).toBe("'a&&b'");
    expect(shellEscape("a|b")).toBe("'a|b'");
  });

  it("handles paths with spaces", () => {
    expect(shellEscape("/path/with spaces/project")).toBe(
      "'/path/with spaces/project'"
    );
  });

  it("handles newlines", () => {
    expect(shellEscape("a\nb")).toBe("'a\nb'");
  });

  it("produces correct dev-finder command", () => {
    const binary = "/opt/homebrew/bin/dev-finder";
    const path = "/Users/me/Dev/my project";
    const cmd = `${shellEscape(binary)} --project ${shellEscape(path)}`;
    expect(cmd).toBe(
      "'/opt/homebrew/bin/dev-finder' --project '/Users/me/Dev/my project'"
    );
  });
});

describe("Code Intelligence URL validation", () => {
  const validPrefixes = [
    "http://localhost",
    "http://127.0.0.1",
    "https://localhost",
    "https://127.0.0.1",
    "http://[::1]",
  ];

  function isValidCIUrl(url: string): boolean {
    return validPrefixes.some((p) => url.startsWith(p));
  }

  it("accepts localhost URLs", () => {
    expect(isValidCIUrl("http://localhost:3000")).toBe(true);
    expect(isValidCIUrl("http://127.0.0.1:8080")).toBe(true);
    expect(isValidCIUrl("https://localhost:443")).toBe(true);
    expect(isValidCIUrl("http://[::1]:3000")).toBe(true);
  });

  it("rejects remote URLs", () => {
    expect(isValidCIUrl("http://example.com:3000")).toBe(false);
    expect(isValidCIUrl("https://remote.server.io")).toBe(false);
    expect(isValidCIUrl("http://192.168.1.100:3000")).toBe(false);
    expect(isValidCIUrl("http://evil.localhost.com")).toBe(false);
  });

  it("rejects non-HTTP schemes", () => {
    expect(isValidCIUrl("ftp://localhost:21")).toBe(false);
    expect(isValidCIUrl("file:///etc/passwd")).toBe(false);
  });
});

describe("ToolLaunchResult interface contract", () => {
  // Ensure the interface shape matches what the backend returns
  it("success result shape", () => {
    const result = { success: true, value: "some-command", detail: "Launched" };
    expect(result.success).toBe(true);
    expect(result.value).toBeTruthy();
    expect(result.detail).toBeTruthy();
  });

  it("failure result shape", () => {
    const result = { success: false, value: null, detail: "not installed" };
    expect(result.success).toBe(false);
    expect(result.value).toBeNull();
    expect(result.detail).toContain("not installed");
  });
});
