use std::io;
use std::net::ToSocketAddrs;

use nginx_config::ast;
use url::{self, Url, Host};
use {Config};

#[derive(Fail, Debug)]
pub enum Error {
    #[fail(display="Url {:?} is invalid: {}", _0, _1)]
    InvalidUrl(String, url::ParseError),
    #[fail(display="Can't resolve {:?} in url {:?}: {}", _0, _1, _2)]
    Resolve(String, String, io::Error),
    #[doc(hidden)]
    #[fail(display="unreachable")]
    __Nonexhaustive,
}

pub fn check_hostnames(cfg: &Config)
    -> Result<(), Vec<Error>>
{
    check_selected_hostnames(cfg, |_| true)
}

pub fn check_selected_hostnames(cfg: &Config,
    mut do_check: impl FnMut(&str) -> bool)
    -> Result<(), Vec<Error>>
{
    use self::Error::*;
    let mut errors = Vec::new();;
    for dir in cfg.all_directives() {
        match dir.item {
            ast::Item::ProxyPass(ref texturl) => {
                let texturl = texturl.to_string();
                let url = match Url::parse(&texturl) {
                    Ok(url) => url,
                    Err(e) => {
                        errors.push(InvalidUrl(texturl, e));
                        continue;
                    }
                };
                match url.host() {
                    Some(Host::Domain(val)) => {
                        if !do_check(val) {
                            continue;
                        }
                        match (val, url.port().unwrap_or(80)).to_socket_addrs()
                        {
                            Ok(_) => {}
                            Err(e) => {
                                errors.push(Resolve(val.to_string(),
                                    texturl, e));
                            }
                        }
                    }
                    Some(Host::Ipv4(..)) => {}
                    Some(Host::Ipv6(..)) => {}
                    None => {}
                }
            }
            _ => {}
        }
    }
    if errors.is_empty() {
        return Ok(());
    }
    return Err(errors);
}
