use std::collections::HashMap;
use std::path::PathBuf;

use failure::Error;
use failure::err_msg;
use nginx_config::ast::{self, Listen, Value};
use nginx_config::parse_directives;
use nginx_config::visitors::{replace_vars, visit_mutable};

use nginx_config_mod::{Config, EntryPoint};
use nginx_config_mod::checks;

#[derive(StructOpt)]
pub struct Modify {

    #[structopt(parse(from_os_str))]
    file: PathBuf,

    #[structopt(short="s", long="subst-variable", name="var=value",
                help="replace variable in the config to specified value")]
    set_var: Vec<String>,

    #[structopt(long="subst-server-name", name="orig.domain=dest.domain",
                help="replace orig.domain and all names starting with it \
                      to a dest.domain (keeping prefix if needed)")]
    server_name_mapping: Vec<String>,

    #[structopt(long="subst-proxy-pass-host", name="orig.host=dest.host",
                help="replace orig.host and all names starting with it \
                      to a dest.host (keeping prefix and port)")]
    proxy_pass_mapping: Vec<String>,

    #[structopt(long="check-proxy-pass-hostnames", help="\
        Also check that all hostnames in proxy_pass directives can be \
        resolved. This is needed because nginx refuses to start if can't \
        resolve IP addresses. \
        Note: this might be slow. \
        ")]
    check_proxy_pass_hostnames: bool,

    #[structopt(long="listen", name="LISTEN",
                help="replace all listen directives to this value",
                parse(try_from_str="parse_listen"))]
    listen: Option<Listen>,
}

fn parse_listen(s: &str) -> Result<Listen, Error> {
    let text = format!("listen {};", s);
    let mut dirs = parse_directives(&text)?;
    if dirs.len() > 1 {
        bail!("Only single listen directive may be specified \
               (consider removing semicolon from argument)");
    }
    match dirs.pop().map(|d| d.item) {
        Some(ast::Item::Listen(lst)) => Ok(lst),
        _ => bail!("Internal error when parsing listen directive"),
    }
}

// TODO(tailhook) temporary, until we expose Value::parse
fn parse_proxy(s: &str) -> Result<Value, Error> {
    let text = format!("proxy_pass {};", s);
    let mut dirs = parse_directives(&text)?;
    if dirs.len() > 1 {
        bail!("Only single proxy_pass directive may be specified \
               (consider removing semicolon from argument)");
    }
    match dirs.pop().map(|d| d.item) {
        Some(ast::Item::ProxyPass(value)) => Ok(value),
        _ => bail!("Internal error when parsing proxy_pass directive"),
    }
}

fn relative<'x>(name: &'x str, anchor: &str) -> Option<&'x str> {
    if name.ends_with(anchor) {
        if anchor.len() == name.len() {
            return Some("");
        } else if name[..name.len() - anchor.len()].ends_with(".") {
            return Some(&name[..name.len() - anchor.len()])
        } else {
            return None
        }
    } else {
        None
    }
}

fn proxy_subst<'x>(name: &'x str, anchor: &str) -> Option<(&'x str, &'x str)> {
    let mut cur = name;
    let mut prefix = 0;
    let mut suffix = 0;
    if name.starts_with("http://") {
        cur = &cur["http://".len()..];
        prefix += "http://".len();
    } else if name.starts_with("https://") {
        cur = &cur["https://".len()..];
        prefix += "https://".len();
    } else {
        return None;
    }
    if let Some(suf) = cur.find('/') {
        suffix += cur.len() - suf;
        cur = &cur[..suf];
    }
    if let Some(suf) = cur.find(':') {
        suffix += cur.len() - suf;
        cur = &cur[..suf];
    }
    if cur.ends_with(anchor) {
        // TODO(tailhook) check for variables ?
        if anchor.len() == cur.len() {
            return Some((&name[..prefix], &name[name.len() - suffix..]));
        } else if cur[..cur.len() - anchor.len()].ends_with(".") {
            prefix += cur.len() - anchor.len();
            return Some((&name[..prefix], &name[name.len() - suffix..]));
        } else {
            return None
        }
    } else {
        None
    }
}


fn server_names(cfg: &mut Config, names: &Vec<String>)
    -> Result<(), Error>
{
    use nginx_config::ast::ServerName::*;
    let mut snames = HashMap::new();
    for item in names {
        let mut pair = item.splitn(2, '=');
        let orig = pair.next().expect("first item always exists");
        if let Some(dest) = pair.next() {
            snames.insert(orig, dest);
        } else {
            bail!("server name {:?} doesn't include substitution target \
                   (format is `orig.name=dest.example.org`)");
        }
    }
    visit_mutable(cfg.directives_mut(), |dir| {
        match dir.item {
            ast::Item::ServerName(ref mut names) => {
                for name in names {
                    match *name {
                        Exact(ref mut n) | Suffix(ref mut n) |
                        StarSuffix(ref mut n)
                        => {
                            for (orig, new) in &snames {
                                *n = if let Some(prefix) = relative(n, orig) {
                                    format!("{}{}", prefix, new)
                                } else {
                                    continue;
                                }
                            }
                        }
                        StarPrefix(_) => {}  // ingoring, warn? forbid?
                        Regex(_) => {}  // ingoring, warn? forbid?
                    }
                }
            }
            _ => {}
        }
    });
    Ok(())
}

fn proxy_pass_mapping(cfg: &mut Config, names: &Vec<String>)
    -> Result<(), Error>
{
    let mut pnames = HashMap::new();
    for item in names {
        let mut pair = item.splitn(2, '=');
        let orig = pair.next().expect("first item always exists");
        if let Some(dest) = pair.next() {
            pnames.insert(orig, dest);
        } else {
            bail!("proxy pass host {:?} doesn't include substitution \
                target (format is `orig.host=dest.example.org`)");
        }
    }
    let mut err = None;
    visit_mutable(cfg.directives_mut(), |dir| {
        match dir.item {
            ast::Item::ProxyPass(ref mut value) => {
                let mut s = value.to_string();
                for (orig, new) in &pnames {
                    s = if let Some((pre, suf)) = proxy_subst(&s, orig) {
                        format!("{}{}{}", pre, new, suf)
                    } else {
                        continue;
                    }
                }
                match parse_proxy(&s) {
                    Ok(x) => *value = x,
                    Err(e) => err = Some(e),
                }
            }
            _ => {}
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    Ok(())
}


pub fn run(modify: Modify) -> Result<(), Error> {
    let mut cfg = Config::partial_file(EntryPoint::Main, &modify.file)?;

    // vars
    let mut vars = HashMap::new();
    for item in &modify.set_var {
        let mut pair = item.splitn(2, '=');
        vars.insert(pair.next().expect("first item always exists"),
            pair.next().unwrap_or(""));
    }
    replace_vars(cfg.directives_mut(), |name| vars.get(name).map(|x| *x));

    // listen
    if let Some(new_listen) = modify.listen {
        visit_mutable(cfg.directives_mut(), |dir| {
            match dir.item {
                ast::Item::Listen(ref mut lst) => *lst = new_listen.clone(),
                _ => {}
            }
        });
    }

    if modify.server_name_mapping.len() > 0 {
        server_names(&mut cfg, &modify.server_name_mapping)?;
    }

    if modify.proxy_pass_mapping.len() > 0 {
        proxy_pass_mapping(&mut cfg, &modify.proxy_pass_mapping)?;
    }

    if modify.check_proxy_pass_hostnames {
        if let Err(errs) = checks::proxy_pass::check_hostnames(&cfg) {
            for e in errs {
                error!("{}", e);
            }
            return Err(err_msg("failed to resolve some hostnames"));
        }
    }

    print!("{}", cfg);
    Ok(())
}
