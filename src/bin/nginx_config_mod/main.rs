extern crate env_logger;
#[macro_use] extern crate structopt;

use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt)]
#[structopt(name = "nginx-config-mod",
            about = "nginx config validation and modification tool")]
enum Options {
    #[structopt(name = "validate", about="Validate nginx configuration")]
    Validate {
        #[structopt(parse(from_os_str))]
        file: PathBuf,
    },
}

fn main() {
    let opt = Options::from_args();
    env_logger::init();
}
