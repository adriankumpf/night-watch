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
use sun::{Event, Sun};

/// How long before a predicted sun event to start polling the camera.
const LEAD_TIME: TimeDelta = TimeDelta::minutes(45);

/// How long to back off when HA reports an event that has already passed.
const STALE_EVENT_BACKOFF: Duration = Duration::from_secs(5);

/// How long after a sun event a fresh start still handles it. A restart while
/// the event was being polled for would otherwise skip it, as HA already
/// reports the following one. Nothing records whether the event already went
/// out, so a restart inside the window after it did sends it again, undoing
/// whatever was changed by hand in the meantime. The window is kept short for
/// that reason, with room above the latest event seen in production (+16 min).
const CATCH_UP_WINDOW: TimeDelta = TimeDelta::hours(1);

/// How long HA and the camera get to answer at startup. Past this, failures
/// are retried for as long as it takes, which would otherwise turn a wrong
/// URL or entity into an endless stream of warnings instead of an exit. It
/// leaves HA time to finish starting when both come up together.
const STARTUP_GRACE: Duration = Duration::from_secs(5 * 60);

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

    /// Polling interval (in seconds), also the pause before retrying a failure
    #[arg(
        short = 'I',
        long,
        default_value = "30",
        value_parser = clap::value_parser!(u16).range(1..),
        display_order = 1
    )]
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
    let pause = Duration::from_secs(args.interval.into());
    let fetch_sun = || persist("Fetching sun.sun", pause, || Sun::fetch(ha));
    let ha_event = |event| match event {
        Event::Sunrise(_) => args.day_event.as_str(),
        Event::Sunset(_) => args.night_event.as_str(),
    };

    let sun = time::timeout(STARTUP_GRACE, async {
        let sun = fetch_sun().await?;
        persist("Checking the camera", pause, || cam.night_vision()).await?;
        anyhow::Ok(sun)
    })
    .await
    .context("HA or the camera kept failing since startup, see the warnings above")??;

    let mut last_event: Option<Event> = None;

    if let Some(event) = sun.previous_event(Utc::now(), CATCH_UP_WINDOW) {
        info!(
            "Catching up on the {event} {} min ago",
            (Utc::now() - event.at()).num_minutes()
        );

        handle(ha, cam, event, ha_event(event), pause).await?;
        last_event = Some(event);
    }

    'main: loop {
        for event in fetch_sun().await?.next_events() {
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

            handle(ha, cam, event, ha_event(event), pause).await?;
            last_event = Some(event);
        }
    }
}

/// Polls the camera every `pause` until it matches `event`, then sends
/// `ha_event` to HA.
///
/// Fails only on [an error that retrying cannot fix][home_assistant::is_permanent].
/// Anything else is retried, since giving up here is what drops the event:
/// by the time a restarted process asks, HA already reports the next one.
/// That includes sending the event, which HA may have fired before the
/// response got lost. A repeated event is the lesser evil.
async fn handle(
    ha: &HomeAssistant,
    cam: &Camera<'_>,
    event: Event,
    ha_event: &str,
    pause: Duration,
) -> Result<()> {
    loop {
        let night_vision = persist(
            &format!("Polling the camera for the {event}"),
            pause,
            || cam.night_vision(),
        )
        .await?;

        match (event, night_vision) {
            (Event::Sunrise(_), false) | (Event::Sunset(_), true) => break,
            _ => time::sleep(pause).await,
        }
    }

    let result = persist(&format!("Sending {ha_event}"), pause, || {
        ha.send_event(ha_event)
    })
    .await?;
    let late = (Utc::now() - event.at()).num_minutes();

    info!("{} [{late:+}]", result.message);

    Ok(())
}

/// Runs `attempt` until it succeeds or fails permanently, sleeping `pause`
/// between attempts and logging each failure, prefixed with `what`.
///
/// This sits on top of the opt-in `--retry` middleware, which gives up on the
/// 404s HA answers while it starts up or reloads an integration and only
/// covers the HTTP exchange, not decoding what came back.
async fn persist<T, F>(what: &str, pause: Duration, mut attempt: impl FnMut() -> F) -> Result<T>
where
    F: Future<Output = Result<T>>,
{
    loop {
        match attempt().await {
            Ok(value) => return Ok(value),
            Err(error) if home_assistant::is_permanent(&error) => return Err(error),
            Err(error) => {
                warn!("{what} failed, trying again in {pause:?}: {error:#}");
                time::sleep(pause).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, FromArgMatches};
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::test_support::{home_assistant, jpeg_response, json_response, solid};

    const PAUSE: Duration = Duration::from_millis(1);

    /// Serves the camera frames `failures` first, then a night vision one,
    /// and accepts the night event.
    async fn serve_failing_camera(failures: Vec<ResponseTemplate>) -> MockServer {
        let server = MockServer::start().await;

        for failure in failures {
            Mock::given(path("/api/camera_proxy/camera.terrasse"))
                .respond_with(failure)
                .up_to_n_times(1)
                .with_priority(1)
                .expect(1)
                .mount(&server)
                .await;
        }

        Mock::given(path("/api/camera_proxy/camera.terrasse"))
            .respond_with(jpeg_response(&solid(16, 16, [40, 40, 40])))
            .with_priority(2)
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/api/events/close_rollershutters"))
            .respond_with(json_response(r#"{"message": "Event fired."}"#))
            .expect(1)
            .mount(&server)
            .await;

        server
    }

    /// What took the process down twice on sunset in production: the camera
    /// entity vanishing for a moment, and a frame that is no image.
    #[tokio::test]
    async fn the_event_is_sent_once_the_camera_recovers() {
        let server = serve_failing_camera(vec![
            ResponseTemplate::new(404),
            ResponseTemplate::new(503),
            ResponseTemplate::new(200),
        ])
        .await;

        let ha = home_assistant(&server);
        let cam = Camera::new(&ha, Source::new("terrasse".into(), false));
        let sunset = Event::Sunset(Utc::now());

        handle(&ha, &cam, sunset, "close_rollershutters", PAUSE)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn sending_the_event_is_retried() {
        let server = serve_failing_camera(vec![]).await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(502))
            .up_to_n_times(1)
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;

        let ha = home_assistant(&server);
        let cam = Camera::new(&ha, Source::new("terrasse".into(), false));

        handle(
            &ha,
            &cam,
            Event::Sunset(Utc::now()),
            "close_rollershutters",
            PAUSE,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_rejected_token_is_not_retried() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;

        let ha = home_assistant(&server);
        let error = persist("Fetching sun.sun", PAUSE, || Sun::fetch(&ha))
            .await
            .err()
            .unwrap();

        assert!(error.to_string().contains("401"), "{error}");
    }

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

    /// Every failure is retried after one interval, so 0 would spin.
    #[test]
    fn a_zero_interval_is_rejected() {
        assert!(parse(&["-T", "t", "-I", "0", "e"]).is_err());
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
