# night-watch

A utility to detect when an IP camera activates or deactivates its night vision mode using a [Home Assistant](https://www.home-assistant.io/) camera feed.

This application is designed to automatically determine the optimal time to open/close shutters based on lighting conditions.

## Usage

```man
Usage: night-watch [OPTIONS] <--token <TOKEN>|--token-file <PATH>> <ENTITY>

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
      --token-file <PATH>          File holding the access token for HA, e.g. a mounted Docker secret [env: TOKEN_FILE]
  -h, --help                       Print help
  -V, --version                    Print version
```

### The token

Supply it one way or the other, not both. Prefer `TOKEN_FILE`, which names a file
whose contents are the token: an environment variable is readable by anything
that can reach the container runtime, through `docker inspect` or
`/proc/<pid>/environ`, and a mounted file is not. It is also what Docker secrets
and systemd credentials already produce.

Surrounding whitespace is stripped, so the trailing newline your editor leaves
behind is not part of the token. A token that is missing, empty or not printable
ASCII is a startup error rather than a 401 an hour later, and so is setting both
forms, which is what you want the day one is rotated and the other is not.

The container runs as uid 10001, and Compose bind-mounts a `file:` secret with
its permissions on the host unchanged, so `chmod 0444 secrets/ha_token` (and rely
on the directory to keep it private) or `chown` it to 10001.

## Deployment

Multi-arch images (`linux/amd64` and `linux/arm64`) are published to
`ghcr.io/adriankumpf/night-watch:latest`.

```yaml
services:
  night-watch:
    image: ghcr.io/adriankumpf/night-watch:latest
    command: ["--url", "http://192.168.1.42:8123", "camera.driveway"]
    environment:
      TOKEN_FILE: /run/secrets/ha_token
    secrets:
      - ha_token
    restart: unless-stopped

secrets:
  ha_token:
    file: ./secrets/ha_token
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
