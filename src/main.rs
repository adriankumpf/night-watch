use structopt::StructOpt;

use anyhow::Result;
use reqwest::Url;

mod client;

#[derive(StructOpt, Debug)]
#[structopt(name = "sun-events")]
struct Args {
    /// The camera (HA entitiy)
    #[structopt(short, long)]
    camera: String,

    /// The HA url
    #[structopt(short, long, default_value = "http://localhost:8123")]
    url: Url,

    /// The access token for HA
    #[structopt(short, long)]
    token: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::from_args();

    let client = client::Client::new(args.url, &args.token)?;
    dbg!(client.get_sun::<client::Sun>("sun.sun").await?);

    Ok(())
}
