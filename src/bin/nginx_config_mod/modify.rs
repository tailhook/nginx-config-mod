use std::collections::HashMap;
use std::path::PathBuf;

use nginx_config_mod::{Config, EntryPoint};
use nginx_config::parse_directives;
use nginx_config::ast::{self, Listen};
use nginx_config::visitors::{replace_vars, visit_mutable};

use failure::Error;

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

    // servernames
    if modify.server_name_mapping.len() > 0 {
        use nginx_config::ast::ServerName::*;
        let mut snames = HashMap::new();
        for item in &modify.server_name_mapping {
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
    }

    print!("{}", cfg);
    Ok(())
}
