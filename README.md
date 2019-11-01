# night-watch

A [Home Assistant](https://www.home-assistant.io/) extension to detect when an IP camera (de)activates its night vision.

## Background

This application has been developed to automatically determine the optimal time to open/close roller shutters – depending on the lighting conditions.

## Usage

```man
night-watch 0.1.0

USAGE:
    night-watch [FLAGS] [OPTIONS] <entity> -T <token>

FLAGS:
    -d, --debug              Activates debug mode
    -s, --from-select        Fetches the camera entity from an input_select element instead
    -h, --help               Prints help information
    -t, --test-connection    Tests the connection to HA and blocks until it is available
    -V, --version            Prints version information

OPTIONS:
    -I <interval>           Polling interval (in seconds) [default: 30]
    -D <day-event>          Event sent to HA when the camera turns off night vision [default: open_rollershutters]
    -N <night-event>        Event sent to HA when the camera turns on night vision [default: close_rollershutters]
    -T <token>              Access token for HA [env: TOKEN]
    -U <url>                Base URL of HA [default: http://localhost:8123]

ARGS:
    <entity>    Entity
```

## Requirements

- Rust 1.39
