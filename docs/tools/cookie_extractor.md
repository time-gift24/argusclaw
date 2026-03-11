# Cookie Extractor Tool

Extract cookies from Chrome for a specific domain.

## Prerequisites

Start Chrome with remote debugging:
```bash
chrome --remote-debugging-port=9222
```

## Usage

### Get CDP URL
```bash
curl http://localhost:9222/json
# Look for "webSocketDebuggerUrl"
```

### Extract Cookies
```json
{
  "action": "cookie_extractor",
  "cdpUrl": "ws://localhost:9222/devtools/browser/ABC123",
  "domain": "example.com"
}
```

### Response
```json
{
  "domain": "example.com",
  "count": 2,
  "cookies": [
    {
      "name": "session",
      "value": "abc123",
      "domain": ".example.com",
      "path": "/",
      "secure": true,
      "httpOnly": true
    }
  ]
}
```

## Notes

- Domain matching uses substring containment (`.example.com` matches `example.com`)
- Requires Chrome to be running
- No authentication — works only with local CDP
