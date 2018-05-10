use std::path::PathBuf;

use failure::{Error, err_msg};
use nginx_config_mod::{Config, EntryPoint, checks};

#[derive(StructOpt)]
pub struct Validate {
    #[structopt(parse(from_os_str))]
    file: PathBuf,

    #[structopt(long="check-proxy-pass-hostnames", help="\
        Also check that all hostnames in proxy_pass directives can be \
        resolved. This is needed because nginx refuses to start if can't \
        resolve IP addresses. \
        Note: this might be slow. \
        ")]
    check_proxy_pass_hostnames: bool,
}

pub fn run(validate: Validate) -> Result<(), Error> {
    let cfg = Config::partial_file(EntryPoint::Main, &validate.file)?;
    if validate.check_proxy_pass_hostnames {
        if let Err(errs) = checks::proxy_pass::check_hostnames(&cfg) {
            for e in errs {
                error!("{}", e);
            }
            return Err(err_msg("failed to resolve some hostnames"));
        }
    }
    Ok(())
}
