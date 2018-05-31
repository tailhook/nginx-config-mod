extern crate regex;
extern crate env_logger;
extern crate nginx_config;
extern crate nginx_config_mod;
#[macro_use] extern crate failure;
#[macro_use] extern crate log;
#[macro_use] extern crate structopt;

mod modify;
mod validate;

use std::path::PathBuf;
use std::process::exit;

use failure::Error;
use structopt::StructOpt;
use nginx_config_mod::{Config, EntryPoint};

use modify::Modify;
use validate::Validate;


#[derive(StructOpt)]
#[structopt(name = "nginx-config-mod",
            about = "nginx config validation and modification tool")]
enum Options {
    #[structopt(name = "validate", about="Validate nginx configuration")]
    Validate(Validate),

    #[structopt(name = "format",
                about="Format (prettify) nginx configuration")]
    Format {
        #[structopt(parse(from_os_str))]
        file: PathBuf,
    },

    #[structopt(name="modify",
                about="Apply various modifications to config")]
    Modify(Modify),
}

fn run(opt: Options) -> Result<(), Error> {
    use self::Options::*;
    match opt {
        Validate(validate) => {
            validate::run(validate)?
        }
        Format { file } => {
            let cfg = Config::partial_file(EntryPoint::Main, &file)?;
            print!("{}", cfg);
        }
        Modify(modify) => {
            modify::run(modify)?
        }
    }
    Ok(())
}

fn main() {
    let opt = Options::from_args();
    env_logger::init();
    match run(opt) {
        Ok(()) => {}
        Err(e) => {
            error!("{}", e);
            exit(1);
        }
    }
}
