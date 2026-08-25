mod camera;
mod home_assistant;
mod sun;
#[cfg(test)]
mod test_support;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use chrono::{TimeDelta, offset::Utc};
use clap::{ArgGroup, Parser, crate_version};
use reqwest::Url;
use tokio::time;
use tracing::{debug, info, warn};
use tracing_subscriber::filter::{EnvFilter, LevelFilter};
use tracing_subscriber::fmt;

use camera::{Camera, Source};
use home_assistant::HomeAssistant;
use sun::Event;

/// How long before a predicted sun event to start polling the camera.
const LEAD_TIME: TimeDelta = TimeDelta::minutes(45);

/// How long to back off when HA reports an event that has already passed.
const STALE_EVENT_BACKOFF: Duration = Duration::from_secs(5);

// No `Debug`: `token` holds the secret, so deriving one would put it a
// `{args:?}` away from a log line.
#[derive(Parser)]
#[command(version = crate_version!())]
#[command(group = ArgGroup::new("credentials").required(true).args(["token", "token_file"]))]
struct Args {
    /// Print debug logs
    #[arg(short, long)]
    debug: bool,

    /// Retry failed requests with increasing intervals between attempts (up to 2 minutes)
    #[arg(short, long)]
    retry: bool,

    /// Fetches the camera entity from an input_select element instead
    #[arg(short = 's', long)]
    from_select: bool,

    /// Polling interval (in seconds)
    #[arg(short = 'I', long, default_value = "30", display_order = 1)]
    interval: u16,

    /// Event sent to HA when the camera turns on night vision
    #[arg(
        short = 'N',
        long,
        default_value = "close_rollershutters",
        display_order = 2
    )]
    night_event: String,

    /// Event sent to HA when the camera turns off night vision
    #[arg(
        short = 'D',
        long,
        default_value = "open_rollershutters",
        display_order = 2
    )]
    day_event: String,

    /// Base URL of HA
    #[arg(short = 'U', long, default_value = "http://localhost:8123")]
    url: Url,

    /// Access token for HA
    #[arg(short = 'T', long, env = "TOKEN", hide_env_values = true)]
    token: Option<String>,

    /// File holding the access token for HA, e.g. a mounted Docker secret
    #[arg(long, env = "TOKEN_FILE", hide_env_values = true, value_name = "PATH")]
    token_file: Option<PathBuf>,

    /// Entity
    #[arg()]
    entity: String,
}

impl Args {
    /// The token, from whichever of the two arguments the group let through.
    ///
    /// HA issues JWTs, so trimming cannot damage a real token and it spares us
    /// the trailing newline every convenient way of writing a secret file leaves
    /// behind. What is left still has to survive `HeaderValue::from_str`, which
    /// reports a stray byte as an opaque `failed to parse header value`. Neither
    /// error quotes the token.
    fn token(&self) -> Result<String> {
        let (source, contents) = match (&self.token, &self.token_file) {
            (Some(token), _) => ("--token".to_owned(), token.clone()),
            (_, Some(path)) => {
                let source = format!("--token-file {}", path.display());
                let contents = std::fs::read_to_string(path).with_context(|| source.clone())?;
                (source, contents)
            }
            _ => unreachable!("the argument group is required"),
        };

        let token = contents.trim();

        ensure!(!token.is_empty(), "{source} is empty");
        ensure!(
            token.chars().all(|c| c.is_ascii_graphic()),
            "{source} holds something other than printable ASCII"
        );

        Ok(token.to_owned())
    }
}

fn init_logger(debug: bool) {
    let format = fmt::format().without_time().with_target(false).compact();

    let level = if debug {
        LevelFilter::DEBUG
    } else {
        LevelFilter::INFO
    };

    let filter = EnvFilter::builder()
        .with_default_directive(level.into())
        .parse_lossy("");

    tracing_subscriber::fmt()
        .event_format(format)
        .with_env_filter(filter)
        .init();
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    init_logger(args.debug);

    let token = args.token()?;
    let ha = HomeAssistant::new(args.url.clone(), &token, args.retry)?;
    let cam = Camera::new(&ha, Source::new(args.entity.clone(), args.from_select));

    tokio::select! {
        result = watch(&ha, &cam, &args) => result,
        () = shutdown_signal()? => Ok(()),
    }
}

/// Registers the termination signals up front so a missing handler fails
/// startup, and resolves once either one arrives.
///
/// Not optional: the container runs this binary as PID 1, where the kernel
/// drops a default-disposition SIGTERM, so without a handler `docker stop`
/// waits out its timeout and lands on SIGKILL.
fn shutdown_signal() -> Result<impl Future<Output = ()>> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;

    Ok(async move {
        let signal = tokio::select! {
            _ = interrupt.recv() => "SIGINT",
            _ = terminate.recv() => "SIGTERM",
        };

        info!("received {signal}, shutting down");
    })
}

async fn watch(ha: &HomeAssistant, cam: &Camera<'_>, args: &Args) -> Result<()> {
    let mut last_event: Option<Event> = None;

    'main: loop {
        for event in sun::next_events(ha).await? {
            if last_event.is_some_and(|last| last.same_kind(event)) {
                debug!("{event} was already handled!");
                continue;
            }

            let event_in = event.at() - Utc::now();

            info!(
                "Next {event} in {:.1} hours",
                event_in.num_minutes() as f32 / 60.0
            );

            if event_in <= TimeDelta::zero() {
                warn!("The {event} is in the past");
                time::sleep(STALE_EVENT_BACKOFF).await;
                continue 'main;
            }

            if let Ok(sleep_for) = (event_in - LEAD_TIME).to_std() {
                time::sleep(sleep_for).await;
            }

            info!("{event} in {} min", (event.at() - Utc::now()).num_minutes());

            let ha_event = loop {
                let night_vision = cam.night_vision().await?;

                match (event, night_vision) {
                    (Event::Sunrise(_), false) => break &args.day_event,
                    (Event::Sunset(_), true) => break &args.night_event,
                    _ => time::sleep(Duration::from_secs(args.interval.into())).await,
                }
            };

            let result = ha.send_event(ha_event).await?;
            let late = (Utc::now() - event.at()).num_minutes();

            info!("{} [{late:+}]", result.message);

            last_event = Some(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, FromArgMatches};
    use tempfile::TempDir;

    use super::*;

    /// Parses with every `env` source detached, so that a `TOKEN` exported in
    /// the developer's own shell cannot change what these tests see.
    fn parse(args: &[&str]) -> Result<Args, clap::Error> {
        let matches = Args::command()
            .mut_args(|arg| arg.env(None))
            .try_get_matches_from(std::iter::once("night-watch").chain(args.iter().copied()))?;

        Args::from_arg_matches(&matches)
    }

    fn token_file(dir: &TempDir, contents: &str) -> String {
        let path = dir.path().join("token");
        std::fs::write(&path, contents).unwrap();
        format!("--token-file={}", path.display())
    }

    #[test]
    fn no_arguments_conflict() {
        Args::command().debug_assert();
    }

    #[test]
    fn unset_arguments_fall_back_to_their_defaults() {
        let args = parse(&["--token", "s3cr3t", "front_door"]).unwrap();

        assert_eq!(args.entity, "front_door");
        assert_eq!(args.url.as_str(), "http://localhost:8123/");
        assert_eq!(args.interval, 30);
        assert_eq!(args.day_event, "open_rollershutters");
        assert_eq!(args.night_event, "close_rollershutters");
        assert!(!args.debug && !args.retry && !args.from_select);
        assert_eq!(args.token().unwrap(), "s3cr3t");
    }

    #[test]
    fn the_entity_is_required() {
        assert!(parse(&["--token", "s3cr3t"]).is_err());
    }

    #[test]
    fn an_invalid_url_is_rejected() {
        assert!(parse(&["-T", "t", "-U", "localhost", "e"]).is_err());
    }

    #[test]
    fn one_form_of_the_token_or_the_other_is_required() {
        assert!(parse(&["front_door"]).is_err());
    }

    /// Preferring one silently would let a rotated-away token sit in a compose
    /// file looking like it is the one in use.
    #[test]
    fn setting_both_forms_of_the_token_is_rejected() {
        let dir = TempDir::new().unwrap();
        let flag = token_file(&dir, "from-the-file");

        assert!(parse(&["--token", "from-the-flag", &flag, "front_door"]).is_err());
    }

    #[test]
    fn the_token_is_the_file_contents_without_the_surrounding_whitespace() {
        let dir = TempDir::new().unwrap();

        for contents in ["s3cr3t", "s3cr3t\n", "s3cr3t\r\n", "  s3cr3t \n\n"] {
            let flag = token_file(&dir, contents);
            let args = parse(&[&flag, "front_door"]).unwrap();

            assert_eq!(args.token().unwrap(), "s3cr3t");
        }
    }

    #[test]
    fn a_missing_token_file_is_rejected() {
        let dir = TempDir::new().unwrap();
        let flag = format!("--token-file={}", dir.path().join("nope").display());

        assert!(parse(&[&flag, "front_door"]).unwrap().token().is_err());
    }

    /// Either way it arrives, and before the request that would fail with a 401
    /// or an opaque `failed to parse header value`.
    #[test]
    fn a_token_that_cannot_work_is_rejected() {
        let dir = TempDir::new().unwrap();

        for token in [
            "",
            "\n",
            "   ",
            "s3c\nr3t",
            "\u{feff}s3cr3t",
            "s3c\u{a0}r3t",
        ] {
            let flag = token_file(&dir, token);
            assert!(parse(&[&flag, "front_door"]).unwrap().token().is_err());
            assert!(
                parse(&["--token", token, "front_door"])
                    .unwrap()
                    .token()
                    .is_err()
            );
        }
    }
}
