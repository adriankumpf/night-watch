# night-watch

```man
night-watch 0.1.0

USAGE:
    night-watch [FLAGS] [OPTIONS] --token <token>

FLAGS:
    -d, --debug      Activate debug mode
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -D, --day-event <day-event>        The open event [default: open_rollershutters]
    -i, --interval <interval>          Polling interval (seconds) [default: 30]
    -N, --night-event <night-event>    The close event [default: close_rollershutters]
    -s, --select <select>              Input select for camera [default: night_watch]
    -t, --token <token>                The access token for HA [env: TOKEN=]
    -u, --url <url>                    The HA url [default: http://localhost:8123]
```
