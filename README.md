`kaiyote`
========

```
HTTP proxy middleware with route interception capabilities

Usage: kaiyote [OPTIONS]

Options:
  -t, --target <TARGET>  Target URL to proxy requests to [default: http://127.0.0.1:8080]
  -b, --bind <BIND>      Address to bind the proxy server to [default: 127.0.0.1:3000]
  -h, --help             Print help
```

Kaiyote forwards HTTP traffic from its `bind` address to the `target`, unless a block rule prevents
it. Block rules are added at runtime via the `/control/` API, see below.

## API

- `POST /control/block?route=X`: block the route X (and all suffixes) with a 500 internal server error.
- `POST /control/unblock?route=X`: unblock the route X (and all suffixes).

## Docker

Temporarily available on Docker Hub under `michaelsproul/kaiyote:latest`.
