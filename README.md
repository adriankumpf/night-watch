# night-watch

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
