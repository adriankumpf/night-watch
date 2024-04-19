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
