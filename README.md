# night-watch

A utility to detect when an IP camera activates or deactivates its night vision mode using a [Home Assistant](https://www.home-assistant.io/) camera feed.

This application is designed to automatically determine the optimal time to open/close shutters based on lighting conditions.

## Usage

```man
Usage: night-watch [OPTIONS] --token <TOKEN> <ENTITY>

Arguments:
  <ENTITY>  Entity

Options:
  -d, --debug                      Print debug logs
  -I, --interval <INTERVAL>        Polling interval (in seconds) [default: 30]
  -r, --retry                      Retry failed requests with increasing intervals between attempts (up to 2 minutes)
  -D, --day-event <DAY_EVENT>      Event sent to HA when the camera turns off night vision [default: open_rollershutters]
  -N, --night-event <NIGHT_EVENT>  Event sent to HA when the camera turns on night vision [default: close_rollershutters]
  -s, --from-select                Fetches the camera entity from an input_select element instead
  -U, --url <URL>                  Base URL of HA [default: http://localhost:8123]
  -T, --token <TOKEN>              Access token for HA [env: TOKEN]
  -h, --help                       Print help
  -V, --version                    Print version
```

## Deployment

Multi-arch images (`linux/amd64` and `linux/arm64`) are published to
`ghcr.io/adriankumpf/night-watch:latest`.

```yaml
services:
  night-watch:
    image: ghcr.io/adriankumpf/night-watch:latest
    command: ["--url", "http://192.168.1.42:8123", "camera.driveway"]
    environment:
      TOKEN: ${TOKEN}
    restart: unless-stopped
```

### Addressing Home Assistant

Use an IP address or a regular DNS name for `--url`. **`homeassistant.local`
will not resolve**, and fails with `dns error: failed to lookup address
information`.

The binary is statically linked against musl, and musl implements no NSS. It
resolves via `/etc/hosts` and plain DNS from `/etc/resolv.conf` only, so mDNS
(`.local`) is unavailable no matter how the host is configured. If you would
rather keep the hostname, map it yourself -- Docker mounts `/etc/hosts` into the
container, and musl does read it:

```yaml
    extra_hosts:
      - "homeassistant.local:192.168.1.42"
```

Note also that `reqwest` is built without a TLS backend, so `--url` must be
`http://`. This keeps the runtime image at ~2 MB (it is built `FROM scratch`),
and is fine for talking to Home Assistant across a trusted LAN. Anything routed
over an untrusted network wants a TLS-terminating proxy in front.
